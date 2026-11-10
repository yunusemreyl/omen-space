// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * hp-omen-extra — Companion driver for HP Omen/Victus RGB keyboard backlight.
 *
 * Works alongside the stock hp-wmi driver (which handles fan hwmon,
 * hotkeys, thermal profiles, rfkill).  This module only manages the
 * per-zone RGB colour of the keyboard backlight via WMI.
 *
 * Copyright (C) 2024 Yunus Emre YILMAZ <yunusemreyl>
 *
 * Based on hp-wmi.c by Matthew Garrett and Anssi Hannula, and on
 * hp-omen-rgb by yunusemreyl.
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/acpi.h>
#include <linux/errno.h>
#include <linux/init.h>
#include <linux/kernel.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/platform_device.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/types.h>
#include <linux/wmi.h>

MODULE_AUTHOR("Yunus Emre <yunusemreyl>");
MODULE_DESCRIPTION("HP Omen/Victus keyboard RGB companion driver");
MODULE_LICENSE("GPL");
/* NOTE: No MODULE_ALIAS("wmi:...") — we do NOT claim any WMI GUID
 * so we can coexist with the stock hp-wmi driver.                 */

/* ── WMI GUID used by hp_wmi_perform_query (shared, not claimed) ── */
#define HPWMI_BIOS_GUID "5FB7F034-2C63-45E9-BE91-3D44E2C707E4"

/* ── WMI command / query types we need ── */
enum hp_wmi_command {
  HPWMI_READ = 0x01,
  HPWMI_WRITE = 0x02,
  HPWMI_BACKLIGHT = 0x20009,
  HPWMI_GAMING_KEY = 0x2000B,
};

enum hp_wmi_backlight_commandtype {
  HPWMI_COLOR_GET_QUERY = 0x02,
  HPWMI_COLOR_SET_QUERY = 0x03,
  HPWMI_BRIGHTNESS_GET_QUERY = 0x04,
  HPWMI_BRIGHTNESS_SET_QUERY = 0x05,
  HPWMI_LIGHTBAR_GET_QUERY = 0x0A,
  HPWMI_LIGHTBAR_SET_QUERY = 0x0B,
};


/* ── WMI query structures and definitions (standalone) ── */
struct bios_args {
	u32 signature;
	u32 command;
	u32 commandtype;
	u32 datasize;
	u8  data[];
};

struct bios_return {
	u32 sigpass;
	u32 return_code;
};

static inline int encode_outsize_for_pvsz(int outsize)
{
	if (outsize > 4096)
		return -EINVAL;
	if (outsize > 1024)
		return 5;
	if (outsize > 128)
		return 4;
	if (outsize > 4)
		return 3;
	if (outsize > 0)
		return 2;
	return 1;
}

static DEFINE_MUTEX(hp_rgb_mutex);

static int hp_wmi_perform_query(int query, enum hp_wmi_command command,
				void *buffer, int insize, int outsize)
{
	struct acpi_buffer input, output = {ACPI_ALLOCATE_BUFFER, NULL};
	struct bios_return *bios_return;
	union acpi_object *obj = NULL;
	struct bios_args *args = NULL;
	int mid, actual_insize;
	size_t bios_args_size;
	int ret;

	mid = encode_outsize_for_pvsz(outsize);
	if (WARN_ON(mid < 0))
		return mid;

	actual_insize = max_t(int, insize, 128);
	bios_args_size = struct_size(args, data, actual_insize);
	args = kzalloc(bios_args_size, GFP_KERNEL);
	if (!args)
		return -ENOMEM;

	input.length  = bios_args_size;
	input.pointer = args;

	args->signature   = 0x55434553;
	args->command     = command;
	args->commandtype = query;
	args->datasize    = insize == 0 ? 4 : insize;

	if (insize > 0)
		memcpy(args->data, buffer, flex_array_size(args, data, insize));

	mutex_lock(&hp_rgb_mutex);
	ret = wmi_evaluate_method(HPWMI_BIOS_GUID, 0, mid, &input, &output);
	mutex_unlock(&hp_rgb_mutex);
	if (ret)
		goto out_free;

	obj = output.pointer;
	if (!obj) {
		ret = -EINVAL;
		goto out_free;
	}

	if (obj->type != ACPI_TYPE_BUFFER) {
		pr_warn("query 0x%x returned an invalid object type 0x%x\n",
			query, obj->type);
		ret = -EINVAL;
		goto out_free;
	}

	if (!obj->buffer.pointer ||
	    obj->buffer.length < sizeof(*bios_return)) {
		pr_warn("query 0x%x returned invalid buffer\n", query);
		ret = -EINVAL;
		goto out_free;
	}

	bios_return = (struct bios_return *)obj->buffer.pointer;
	ret = bios_return->return_code;

	if (ret) {
		/* Don't spam dmesg for unknown commands during probing (e.g. win_lock on Victus) */
		if (ret != 0x3 || query != 0x0)
			pr_warn("query 0x%x returned error 0x%x\n", query, ret);
		goto out_free;
	}

	if (!outsize)
		goto out_free;

	if (outsize > obj->buffer.length - sizeof(*bios_return)) {
		pr_warn("query 0x%x returned buffer too small\n", query);
		ret = -EINVAL;
		goto out_free;
	}

	memcpy(buffer, obj->buffer.pointer + sizeof(*bios_return), outsize);

out_free:
	/* output.pointer is allocated by ACPI (ACPI_ALLOCATE_BUFFER); must
	 * always be freed via kfree(output.pointer), NOT kfree(obj), because
	 * obj may be NULL if we jumped here before the obj = output.pointer
	 * assignment. */
	kfree(output.pointer);
	kfree(args);
	return ret;
}
/* ══════════════════════════════════════════════════════════════════
 * RGB ZONE SYSFS  (zone0 … zone7)
 * echo "FF0000" > /sys/devices/platform/hp-omen-extra/zone0
 * cat  /sys/devices/platform/hp-omen-extra/zone0   → "FF0000"
 * ══════════════════════════════════════════════════════════════════ */
#define RGB_ZONE_COUNT 8
#define COLOR_TABLE_SIZE 128
#define COLOR_OFFSET 25 /* RGB data starts at byte 25 in the table */



static ssize_t zone_show(struct device *dev, struct device_attribute *attr,
                         char *buf) {
  int zone;
  u8 tbl[COLOR_TABLE_SIZE];
  int ret;

  if (kstrtoint(attr->attr.name + 4, 10, &zone) || zone < 0 ||
      zone >= RGB_ZONE_COUNT)
    return -EINVAL;

  memset(tbl, 0, sizeof(tbl));
  ret = hp_wmi_perform_query(HPWMI_COLOR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));

  if (ret)
    return -EIO;

  return sysfs_emit(buf, "%02X%02X%02X\n", tbl[COLOR_OFFSET + zone * 3 + 0],
                    tbl[COLOR_OFFSET + zone * 3 + 1],
                    tbl[COLOR_OFFSET + zone * 3 + 2]);
}

static ssize_t zone_store(struct device *dev, struct device_attribute *attr,
                          const char *buf, size_t count) {
  int zone;
  u32 rgb;
  u8 tbl[COLOR_TABLE_SIZE];
  int ret;
  unsigned int r, g, b;

  if (kstrtoint(attr->attr.name + 4, 10, &zone) || zone < 0 ||
      zone >= RGB_ZONE_COUNT)
    return -EINVAL;

  if (sscanf(buf, "%u %u %u", &r, &g, &b) == 3) {
      if (r > 255 || g > 255 || b > 255)
          return -EINVAL;
      rgb = (r << 16) | (g << 8) | b;
  } else if (kstrtou32(buf, 16, &rgb)) {
      return -EINVAL;
  }

  memset(tbl, 0, sizeof(tbl));
  ret = hp_wmi_perform_query(HPWMI_COLOR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  if (ret) {
    pr_warn("hp-omen-extra: zone%d color GET failed: WMI returned %d\n", zone, ret);
    return -EIO;
  }

  tbl[COLOR_OFFSET + zone * 3 + 0] = (rgb >> 16) & 0xFF;
  tbl[COLOR_OFFSET + zone * 3 + 1] = (rgb >> 8) & 0xFF;
  tbl[COLOR_OFFSET + zone * 3 + 2] = rgb & 0xFF;

  ret = hp_wmi_perform_query(HPWMI_COLOR_SET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  if (ret)
    pr_warn("hp-omen-extra: zone%d color SET failed: WMI returned %d\n", zone, ret);

  return ret ? -EIO : count;
}

/* ── all_zones: batch write/read all zones in a single WMI call ── */
static ssize_t all_zones_show(struct device *dev, struct device_attribute *attr,
                              char *buf) {
  u8 tbl[COLOR_TABLE_SIZE];
  int ret, z, offset = 0;

  memset(tbl, 0, sizeof(tbl));
  ret = hp_wmi_perform_query(HPWMI_COLOR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  if (ret)
    return -EIO;

  for (z = 0; z < RGB_ZONE_COUNT; z++) {
    offset += sysfs_emit_at(buf, offset, "%s%02X%02X%02X",
                            z > 0 ? " " : "",
                            tbl[COLOR_OFFSET + z * 3 + 0],
                            tbl[COLOR_OFFSET + z * 3 + 1],
                            tbl[COLOR_OFFSET + z * 3 + 2]);
  }
  offset += sysfs_emit_at(buf, offset, "\n");
  return offset;
}

static ssize_t all_zones_store(struct device *dev, struct device_attribute *attr,
                               const char *buf, size_t count) {
  u8 tbl[COLOR_TABLE_SIZE];
  int ret, z = 0;
  const char *cur = buf;
  char token[32];

  memset(tbl, 0, sizeof(tbl));
  ret = hp_wmi_perform_query(HPWMI_COLOR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  if (ret)
    return -EIO;

  while (*cur && z < RGB_ZONE_COUNT) {
    while (*cur == ' ' || *cur == '\t' || *cur == ',' || *cur == '\n') cur++;
    if (!*cur) break;
    int len = 0;
    while (cur[len] && cur[len] != ' ' && cur[len] != '\t' && cur[len] != ',' && cur[len] != '\n' && len < 31)
      len++;
    memcpy(token, cur, len);
    token[len] = '\0';
    cur += len;

    u32 rgb = 0;
    if (kstrtou32(token, 16, &rgb) == 0) {
      tbl[COLOR_OFFSET + z * 3 + 0] = (rgb >> 16) & 0xFF;
      tbl[COLOR_OFFSET + z * 3 + 1] = (rgb >> 8) & 0xFF;
      tbl[COLOR_OFFSET + z * 3 + 2] = rgb & 0xFF;
      z++;
    }
  }

  if (z == 0)
    return -EINVAL;

  ret = hp_wmi_perform_query(HPWMI_COLOR_SET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  return ret ? -EIO : count;
}

/* ── lightbar: support 4-zone lightbar controls via query 0x0B / 0x03 ── */
static ssize_t lightbar_show(struct device *dev, struct device_attribute *attr,
                             char *buf) {
  u8 tbl[COLOR_TABLE_SIZE];
  int ret;

  memset(tbl, 0, sizeof(tbl));
  ret = hp_wmi_perform_query(HPWMI_LIGHTBAR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  if (ret) {
    ret = hp_wmi_perform_query(HPWMI_COLOR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                               sizeof(tbl), sizeof(tbl));
  }
  if (ret)
    return -EIO;

  return sysfs_emit(buf, "%02X%02X%02X %02X%02X%02X %02X%02X%02X %02X%02X%02X\n",
                    tbl[COLOR_OFFSET + 0], tbl[COLOR_OFFSET + 1], tbl[COLOR_OFFSET + 2],
                    tbl[COLOR_OFFSET + 3], tbl[COLOR_OFFSET + 4], tbl[COLOR_OFFSET + 5],
                    tbl[COLOR_OFFSET + 6], tbl[COLOR_OFFSET + 7], tbl[COLOR_OFFSET + 8],
                    tbl[COLOR_OFFSET + 9], tbl[COLOR_OFFSET + 10], tbl[COLOR_OFFSET + 11]);
}

static ssize_t lightbar_store(struct device *dev, struct device_attribute *attr,
                              const char *buf, size_t count) {
  u8 tbl[COLOR_TABLE_SIZE];
  int ret, z = 0;
  const char *cur = buf;
  char token[32];

  memset(tbl, 0, sizeof(tbl));
  ret = hp_wmi_perform_query(HPWMI_LIGHTBAR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  if (ret) {
    ret = hp_wmi_perform_query(HPWMI_COLOR_GET_QUERY, HPWMI_BACKLIGHT, tbl,
                               sizeof(tbl), sizeof(tbl));
  }
  if (ret)
    return -EIO;

  while (*cur && z < 4) {
    while (*cur == ' ' || *cur == '\t' || *cur == ',' || *cur == '\n') cur++;
    if (!*cur) break;
    int len = 0;
    while (cur[len] && cur[len] != ' ' && cur[len] != '\t' && cur[len] != ',' && cur[len] != '\n' && len < 31)
      len++;
    memcpy(token, cur, len);
    token[len] = '\0';
    cur += len;

    u32 rgb = 0;
    if (kstrtou32(token, 16, &rgb) == 0) {
      tbl[COLOR_OFFSET + z * 3 + 0] = (rgb >> 16) & 0xFF;
      tbl[COLOR_OFFSET + z * 3 + 1] = (rgb >> 8) & 0xFF;
      tbl[COLOR_OFFSET + z * 3 + 2] = rgb & 0xFF;
      z++;
    }
  }

  if (z == 0)
    return -EINVAL;

  ret = hp_wmi_perform_query(HPWMI_LIGHTBAR_SET_QUERY, HPWMI_BACKLIGHT, tbl,
                             sizeof(tbl), sizeof(tbl));
  if (ret) {
    ret = hp_wmi_perform_query(HPWMI_COLOR_SET_QUERY, HPWMI_BACKLIGHT, tbl,
                               sizeof(tbl), sizeof(tbl));
  }
  return ret ? -EIO : count;
}

/* ── brightness on/off ── */
static ssize_t brightness_show(struct device *dev,
                               struct device_attribute *attr, char *buf) {
  u32 data = 0;
  int ret;

  ret = hp_wmi_perform_query(HPWMI_BRIGHTNESS_GET_QUERY, HPWMI_BACKLIGHT, &data,
                       sizeof(data), sizeof(data));
  
  pr_debug("hp-omen-extra: brightness_show: query returned %d, data 0x%08X\n", ret, data);

  if (ret)
    return ret;

  /* 0xE4 = on, 0x64 = off */
  return sysfs_emit(buf, "%d\n", data == 0xE4 ? 1 : 0);
}

static ssize_t brightness_store(struct device *dev,
                                struct device_attribute *attr, const char *buf,
                                size_t count) {
  unsigned int val;
  u32 data;
  int ret;

  if (kstrtouint(buf, 10, &val))
    return -EINVAL;
  if (val > 1)
    return -EINVAL;

  data = val ? 0xE4 : 0x64;
  
  ret = hp_wmi_perform_query(HPWMI_BRIGHTNESS_SET_QUERY, HPWMI_BACKLIGHT, &data,
                       sizeof(data), 0);
  
  pr_debug("hp-omen-extra: brightness_store(val=%d, data=0x%08X): query returned %d\n", val, data, ret);

  return ret ? ret : count;
}

/* ── gaming key (win lock) ── */
static ssize_t win_lock_show(struct device *dev,
                              struct device_attribute *attr, char *buf) {
  u8 data = 0;
  int ret;

  /*
   * HPWMI_GAMING_KEY is a WMI command value, not a commandtype.
   * We use it as the `command` arg and 0 as the commandtype (read).
   * This matches how the BIOS gaming-key register is accessed.
   */
  ret = hp_wmi_perform_query(0 /* commandtype */, HPWMI_GAMING_KEY, &data,
                       sizeof(data), sizeof(data));

  if (ret == 0x3) /* HPWMI_RET_UNKNOWN_COMMAND */
    return -ENODEV;
  if (ret)
    return ret;

  return sysfs_emit(buf, "%d\n", data & 0x01);
}

static ssize_t win_lock_store(struct device *dev,
                               struct device_attribute *attr, const char *buf,
                               size_t count) {
  unsigned int val;
  u8 data;
  int ret;

  if (kstrtouint(buf, 10, &val))
    return -EINVAL;
  if (val > 1)
    return -EINVAL;

  data = val ? 0x01 : 0x00;

  ret = hp_wmi_perform_query(0 /* commandtype */, HPWMI_GAMING_KEY, &data,
                       sizeof(data), 0);

  return ret ? ret : count;
}



/* ── sysfs attributes ── */
static DEVICE_ATTR(zone0, 0644, zone_show, zone_store);
static DEVICE_ATTR(zone1, 0644, zone_show, zone_store);
static DEVICE_ATTR(zone2, 0644, zone_show, zone_store);
static DEVICE_ATTR(zone3, 0644, zone_show, zone_store);
static DEVICE_ATTR(zone4, 0644, zone_show, zone_store);
static DEVICE_ATTR(zone5, 0644, zone_show, zone_store);
static DEVICE_ATTR(zone6, 0644, zone_show, zone_store);
static DEVICE_ATTR(zone7, 0644, zone_show, zone_store);
static DEVICE_ATTR_RW(all_zones);
static DEVICE_ATTR_RW(lightbar);
static DEVICE_ATTR_RW(brightness);
static DEVICE_ATTR_RW(win_lock);

static struct attribute *hp_omen_extra_attrs[] = {
    &dev_attr_zone0.attr, &dev_attr_zone1.attr, &dev_attr_zone2.attr,
    &dev_attr_zone3.attr, &dev_attr_zone4.attr, &dev_attr_zone5.attr,
    &dev_attr_zone6.attr, &dev_attr_zone7.attr,
    &dev_attr_all_zones.attr, &dev_attr_lightbar.attr,
    &dev_attr_brightness.attr, &dev_attr_win_lock.attr, NULL,
};
ATTRIBUTE_GROUPS(hp_omen_extra);

/* ══════════════════════════════════════════════════════════════════
 * PLATFORM DEVICE
 * ══════════════════════════════════════════════════════════════════ */
static struct platform_device *hp_omen_extra_pdev;

static int __init hp_omen_extra_init(void) {
  int ret;

  pr_info("hp-omen-extra: init starting...\n");

  if (!wmi_has_guid(HPWMI_BIOS_GUID)) {
    pr_info("HP WMI BIOS GUID not found — not an HP system?\n");
    return -ENODEV;
  }
  pr_info("hp-omen-extra: WMI GUID found OK\n");

  hp_omen_extra_pdev = platform_device_register_simple("hp-omen-extra",
                                                 PLATFORM_DEVID_NONE, NULL, 0);
  if (IS_ERR(hp_omen_extra_pdev)) {
    pr_err("hp-omen-extra: platform_device_register_simple failed: %ld\n",
           PTR_ERR(hp_omen_extra_pdev));
    return PTR_ERR(hp_omen_extra_pdev);
  }
  pr_info("hp-omen-extra: platform device registered OK\n");

  ret = sysfs_create_groups(&hp_omen_extra_pdev->dev.kobj, hp_omen_extra_groups);
  if (ret) {
    pr_err("hp-omen-extra: sysfs_create_groups failed: %d\n", ret);
    platform_device_unregister(hp_omen_extra_pdev);
    return ret;
  }

  pr_info("HP Omen/Victus RGB companion driver loaded\n");
  return 0;
}

static void __exit hp_omen_extra_exit(void) {
  sysfs_remove_groups(&hp_omen_extra_pdev->dev.kobj, hp_omen_extra_groups);
  platform_device_unregister(hp_omen_extra_pdev);



  pr_info("HP Omen/Victus RGB companion driver unloaded\n");
}

module_init(hp_omen_extra_init);
module_exit(hp_omen_extra_exit);