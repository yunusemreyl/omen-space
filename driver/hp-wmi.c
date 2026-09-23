// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * HP WMI hotkeys
 *
 * Copyright (C) 2008 Red Hat <mjg@redhat.com>
 * Copyright (C) 2010, 2011 Anssi Hannula <anssi.hannula@iki.fi>
 *
 * Portions based on wistron_btns.c:
 * Copyright (C) 2005 Miloslav Trmac <mitr@volny.cz>
 * Copyright (C) 2005 Bernhard Rosenkraenzer <bero@arklinux.org>
 * Copyright (C) 2005 Dmitry Torokhov <dtor@mail.ru>
 */

#define pr_fmt(fmt) KBUILD_MODNAME ": " fmt

#include <linux/acpi.h>
#include <linux/cleanup.h>
#include <linux/compiler_attributes.h>
#include <linux/dmi.h>
#include <linux/fixp-arith.h>
#include <linux/hwmon.h>
#include <linux/init.h>
#include <linux/input.h>
#include <linux/input/sparse-keymap.h>
#include <linux/kernel.h>
#include <linux/limits.h>
#include <linux/minmax.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/pci.h>
#include <linux/platform_device.h>
#include <linux/platform_profile.h>
#include <linux/power_supply.h>
#include <linux/rfkill.h>
#include <linux/slab.h>
#include <linux/string.h>
#include <linux/types.h>
#include <linux/version.h>
#include <linux/workqueue.h>

MODULE_AUTHOR("Matthew Garrett <mjg59@srcf.ucam.org>");
MODULE_DESCRIPTION("HP laptop WMI driver");
MODULE_LICENSE("GPL");

MODULE_ALIAS("wmi:95F24279-4D7B-4334-9387-ACCDC67EF61C");
MODULE_ALIAS("wmi:5FB7F034-2C63-45E9-BE91-3D44E2C707E4");
MODULE_ALIAS("wmi:1F4C91EB-DC5C-460B-951D-C7CB9B4B8D5E");

#define HPWMI_EVENT_GUID "95F24279-4D7B-4334-9387-ACCDC67EF61C"
#define HPWMI_BIOS_GUID  "5FB7F034-2C63-45E9-BE91-3D44E2C707E4"
#define HPWMI_OMEN_HPC_GUID "1F4C91EB-DC5C-460B-951D-C7CB9B4B8D5E"

static const char *active_bios_guid = HPWMI_BIOS_GUID;

enum hp_ec_offsets {
	HP_EC_OFFSET_UNKNOWN                    = 0x00,
	HP_NO_THERMAL_PROFILE_OFFSET            = 0x01,
	HP_VICTUS_S_EC_THERMAL_PROFILE_OFFSET   = 0x59,
	HP_OMEN_EC_THERMAL_PROFILE_FLAGS_OFFSET = 0x62,
	HP_OMEN_EC_THERMAL_PROFILE_TIMER_OFFSET = 0x63,
	HP_OMEN_EC_THERMAL_PROFILE_OFFSET       = 0x95,
};

#define HP_FAN_SPEED_AUTOMATIC  0x00
#define HP_POWER_LIMIT_DEFAULT  0x00
#define HP_POWER_LIMIT_NO_CHANGE 0xFF
#define HPWMI_MUX_MODE_UMA		BIT(0)
#define HPWMI_MUX_MODE_HYBRID		BIT(1)
#define HPWMI_MUX_MODE_DISCRETE		BIT(2)
#define HPWMI_MUX_MODE_OPTIMUS		BIT(3)
#define HPWMI_MUX_MODE_MASK		GENMASK(6, 0)
#define HPWMI_MUX_LEGACY_MASK		(HPWMI_MUX_MODE_HYBRID | HPWMI_MUX_MODE_DISCRETE)

#define ACPI_AC_CLASS "ac_adapter"

/* Use when zero insize is required */
#define zero_if_sup(tmp) (zero_insize_support ? 0 : sizeof(tmp))

enum hp_thermal_profile_omen_v0 {
	HP_OMEN_V0_THERMAL_PROFILE_DEFAULT     = 0x00,
	HP_OMEN_V0_THERMAL_PROFILE_PERFORMANCE = 0x01,
	HP_OMEN_V0_THERMAL_PROFILE_COOL        = 0x02,
};

enum hp_thermal_profile_omen_v1 {
	HP_OMEN_V1_THERMAL_PROFILE_DEFAULT     = 0x30,
	HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE = 0x31,
	HP_OMEN_V1_THERMAL_PROFILE_COOL        = 0x50,
};

enum hp_thermal_profile_omen_flags {
	HP_OMEN_EC_FLAGS_TURBO   = 0x04,
	HP_OMEN_EC_FLAGS_NOTIMER = 0x02,
	HP_OMEN_EC_FLAGS_JUSTSET = 0x01,
};

enum hp_thermal_profile_victus {
	HP_VICTUS_THERMAL_PROFILE_DEFAULT     = 0x00,
	HP_VICTUS_THERMAL_PROFILE_PERFORMANCE = 0x01,
	HP_VICTUS_THERMAL_PROFILE_QUIET       = 0x03,
};

enum hp_thermal_profile_victus_s {
	HP_VICTUS_S_THERMAL_PROFILE_DEFAULT     = 0x00,
	HP_VICTUS_S_THERMAL_PROFILE_PERFORMANCE = 0x01,
};

#define HP_VICTUS_S_THERMAL_PROFILE_TIMER_SECONDS 120

enum hp_thermal_profile {
	HP_THERMAL_PROFILE_PERFORMANCE = 0x00,
	HP_THERMAL_PROFILE_DEFAULT     = 0x01,
	HP_THERMAL_PROFILE_COOL        = 0x02,
	HP_THERMAL_PROFILE_QUIET       = 0x03,
};

struct thermal_profile_params {
	u8 performance;
	u8 balanced;
	u8 low_power;
	u8 ec_tp_offset;
};

static const struct thermal_profile_params victus_s_thermal_params = {
	.performance  = HP_VICTUS_S_THERMAL_PROFILE_PERFORMANCE,
	.balanced     = HP_VICTUS_S_THERMAL_PROFILE_DEFAULT,
	.low_power    = HP_VICTUS_S_THERMAL_PROFILE_DEFAULT,
	.ec_tp_offset = HP_EC_OFFSET_UNKNOWN,
};

static const struct thermal_profile_params victus_s_known_ec_thermal_params = {
	.performance  = HP_VICTUS_S_THERMAL_PROFILE_PERFORMANCE,
	.balanced     = HP_VICTUS_S_THERMAL_PROFILE_DEFAULT,
	.low_power    = HP_VICTUS_S_THERMAL_PROFILE_DEFAULT,
	.ec_tp_offset = HP_VICTUS_S_EC_THERMAL_PROFILE_OFFSET,
};

static const struct thermal_profile_params omen_v1_thermal_params = {
	.performance  = HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE,
	.balanced     = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.low_power    = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.ec_tp_offset = HP_VICTUS_S_EC_THERMAL_PROFILE_OFFSET,
};

static const struct thermal_profile_params omen_v1_legacy_thermal_params = {
	.performance  = HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE,
	.balanced     = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.low_power    = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.ec_tp_offset = HP_OMEN_EC_THERMAL_PROFILE_OFFSET,
};

static const struct thermal_profile_params omen_v1_no_ec_thermal_params = {
	.performance  = HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE,
	.balanced     = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.low_power    = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.ec_tp_offset = HP_NO_THERMAL_PROFILE_OFFSET,
};

static const struct thermal_profile_params omen_v1_unknown_ec_thermal_params = {
	.performance  = HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE,
	.balanced     = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.low_power    = HP_OMEN_V1_THERMAL_PROFILE_DEFAULT,
	.ec_tp_offset = HP_EC_OFFSET_UNKNOWN,
};

/*
 * A generic pointer for the currently-active board's thermal profile
 * parameters.
 */
static struct thermal_profile_params *active_thermal_profile_params;
static bool has_mux = false;

/*
 * DMI board names of devices that should use the omen specific path for
 * thermal profiles.
 * This was obtained by taking a look in the windows omen command center
 * app and parsing a json file that they use to figure out what capabilities
 * the device should have.
 * A device is considered an omen if the DisplayName in that list contains
 * "OMEN", and it can use the thermal profile stuff if the "Feature" array
 * contains "PerformanceControl".
 */
static const char *const omen_thermal_profile_boards[] = {
	"84DA", "84DB", "84DC", "8572", "8573", "8574", "8575", "8600",
	"8601", "8602", "8603", "8604", "8605", "8606", "8607", "860A",
	"8746", "8747", "8748", "8749", "874A", "8786", "8787", "8788",
	"878A", "878B", "878C", "87B5", "886B", "886C", "88C8", "88CB",
	"88D1", "88D2", "88F4", "88F5", "88F6", "88F7", "88FD", "88FE",
	"88FF", "8900", "8901", "8902", "8912", "8917", "8918", "8949",
	"894A", "89EB", "8A15", "8A18", "8BAD", "8C58", "8E41",
	/*
	 * FIX: 8D41 (HP Omen Max), 8BAC (HP Omen 16-wf0xxx), 8BA9, 8E35,
	 * 8C75 (HP Omen 17-db0xxx), 8C77, 8BCD removed from this list so
	 * they fall through to the Victus S-series thermal profile path,
	 * which correctly handles boards with no EC thermal profile readback.
	 */
};

/*
 * DMI board names of Omen laptops that are specifically set to be thermal
 * profile version 0 by the Omen Command Center app, regardless of what
 * the get system design information WMI call returns.
 */
static const char *const omen_thermal_profile_force_v0_boards[] = {
	"8607", "8746", "8747", "8748", "8749", "874A",
};

/*
 * DMI board names of Omen laptops that have a thermal profile timer which
 * will cause the embedded controller to set the thermal profile back to
 * "balanced" when reaching zero.
 */
static const char *const omen_timed_thermal_profile_boards[] = {
	"8A15",
	"8BAD",
};

/* DMI board names of Victus 16-d laptops */
static const char *const victus_thermal_profile_boards[] = {
	"88F8",
	"8A25",
};

/* DMI board names of Victus 16-r and Victus 16-s laptops */
static const struct dmi_system_id victus_s_thermal_profile_boards[] __initconst = {
	{
		/* 878A: OMEN Laptop 15-ek0xxx */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "878A")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		/* 8DD0: Victus by HP Gaming Laptop 15-fb3xxx */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8DD0")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8902")},
		.driver_data = (void *)&omen_v1_legacy_thermal_params,
	},
	{
		/* 8A13: OMEN by HP Laptop 16-b1xxx */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8A13")},
		.driver_data = (void *)&omen_v1_legacy_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8A42")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8A43")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		/* Victus 15-fb0xxx */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8A3D")},
		.driver_data = (void *)&victus_s_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8A44")},
		.driver_data = (void *)&omen_v1_legacy_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8A4D")},
		.driver_data = (void *)&omen_v1_legacy_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BAB")},
		.driver_data = (void *)&omen_v1_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BBE")},
		.driver_data = (void *)&victus_s_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BC2")},
		.driver_data = (void *)&omen_v1_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BCA")},
		.driver_data = (void *)&omen_v1_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BD4")},
		.driver_data = (void *)&victus_s_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BD5")},
		.driver_data = (void *)&victus_s_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8C76")},
		.driver_data = (void *)&omen_v1_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8C77")},
		.driver_data = (void *)&omen_v1_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8C78")},
		.driver_data = (void *)&omen_v1_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8E35")},
		.driver_data = (void *)&omen_v1_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8C99")},
		.driver_data = (void *)&victus_s_known_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8C9C")},
		.driver_data = (void *)&victus_s_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BCD")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8D41")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8D42")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8D87")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8D88")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BA9")},
		.driver_data = (void *)&omen_v1_legacy_thermal_params,
	},
	{
		/*
		 * 8BAC: HP Omen 16-wf0xxx.  ACPI tables have a broken GETB
		 * helper (CreateField with zero length) that aborts all WMID
		 * methods.  Use no-EC params to skip EC thermal profile reads.
		 */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BAC")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		/*
		 * 8C75: HP Omen 17-db0xxx.  Same broken GETB helper as 8BAC
		 * (AE_AML_BUFFER_LIMIT on _SB.WMID.WMBX / WMBA) causes all
		 * WMID writes to abort silently, leaving fans stuck at 0 RPM
		 * after an overheat.  Use no-EC params to skip EC reads.
		 */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8C75")},
		.driver_data = (void *)&omen_v1_no_ec_thermal_params,
	},
	{
		/* 8BC2: Victus by HP Gaming Laptop 16-r0xxx */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8BC2")},
		.driver_data = (void *)&victus_s_thermal_params,
	},
	{
		/* 8C3F: Victus by HP Gaming Laptop 15-fa1xxx */
		.matches    = {DMI_MATCH(DMI_BOARD_NAME, "8C3F")},
		.driver_data = (void *)&victus_s_thermal_params,
	},
	{},
};

static bool is_victus_s_board;

enum hp_wmi_radio {
	HPWMI_WIFI      = 0x0,
	HPWMI_BLUETOOTH = 0x1,
	HPWMI_WWAN      = 0x2,
	HPWMI_GPS       = 0x3,
};

enum hp_wmi_event_ids {
	HPWMI_DOCK_EVENT             = 0x01,
	HPWMI_PARK_HDD               = 0x02,
	HPWMI_SMART_ADAPTER          = 0x03,
	HPWMI_BEZEL_BUTTON           = 0x04,
	HPWMI_WIRELESS               = 0x05,
	HPWMI_CPU_BATTERY_THROTTLE   = 0x06,
	HPWMI_LOCK_SWITCH            = 0x07,
	HPWMI_LID_SWITCH             = 0x08,
	HPWMI_SCREEN_ROTATION        = 0x09,
	HPWMI_COOLSENSE_SYSTEM_MOBILE = 0x0A,
	HPWMI_COOLSENSE_SYSTEM_HOT   = 0x0B,
	HPWMI_PROXIMITY_SENSOR       = 0x0C,
	HPWMI_BACKLIT_KB_BRIGHTNESS  = 0x0D,
	HPWMI_PEAKSHIFT_PERIOD       = 0x0F,
	HPWMI_BATTERY_CHARGE_PERIOD  = 0x10,
	HPWMI_SANITIZATION_MODE      = 0x17,
	HPWMI_CAMERA_TOGGLE          = 0x1A,
	HPWMI_FN_P_HOTKEY            = 0x1B,
	HPWMI_OMEN_KEY               = 0x1D,
	HPWMI_SMART_EXPERIENCE_APP   = 0x21,
};

/*
 * struct bios_args buffer is dynamically allocated. New WMI command types
 * were introduced that exceed 128-byte data size. Changes to handle
 * the data size allocation scheme were kept in hp_wmi_perform_query function.
 */
struct bios_args {
	u32 signature;
	u32 command;
	u32 commandtype;
	u32 datasize;
	u8  data[];
};

enum hp_wmi_commandtype {
	HPWMI_DISPLAY_QUERY       = 0x01,
	HPWMI_HDDTEMP_QUERY       = 0x02,
	HPWMI_ALS_QUERY           = 0x03,
	HPWMI_HARDWARE_QUERY      = 0x04,
	HPWMI_WIRELESS_QUERY      = 0x05,
	HPWMI_BATTERY_QUERY       = 0x07,
	HPWMI_BIOS_QUERY          = 0x09,
	HPWMI_FEATURE_QUERY       = 0x0b,
	HPWMI_HOTKEY_QUERY        = 0x0c,
	HPWMI_FEATURE2_QUERY      = 0x0d,
	HPWMI_WIRELESS2_QUERY     = 0x1b,
	HPWMI_POSTCODEERROR_QUERY = 0x2a,
	HPWMI_SYSTEM_DEVICE_MODE  = 0x40,
	HPWMI_THERMAL_PROFILE_QUERY = 0x4c,
	HPWMI_GRAPHICS_MUX_QUERY    = 0x52,
};

struct victus_power_limits {
	u8 pl1;
	u8 pl2;
	u8 pl4;
	u8 cpu_gpu_concurrent_limit;
};

struct victus_gpu_power_modes {
	u8 ctgp_enable;
	u8 ppab_enable;
	u8 dstate;
	u8 gpu_slowdown_temp;
};

enum hp_wmi_gm_commandtype {
	HPWMI_FAN_SPEED_GET_QUERY          = 0x11,
	HPWMI_SET_PERFORMANCE_MODE         = 0x1A,
	HPWMI_FAN_SPEED_MAX_GET_QUERY      = 0x26,
	HPWMI_FAN_SPEED_MAX_SET_QUERY      = 0x27,
	HPWMI_GET_SYSTEM_DESIGN_DATA       = 0x28,
	HPWMI_FAN_COUNT_GET_QUERY          = 0x10,
	HPWMI_GET_GPU_THERMAL_MODES_QUERY  = 0x21,
	HPWMI_SET_GPU_THERMAL_MODES_QUERY  = 0x22,
	HPWMI_SET_POWER_LIMITS_QUERY       = 0x29,
	HPWMI_VICTUS_S_FAN_SPEED_GET_QUERY = 0x2D,
	HPWMI_VICTUS_S_FAN_SPEED_SET_QUERY = 0x2E,
	HPWMI_VICTUS_S_GET_FAN_TABLE_QUERY = 0x2F,
	/*
	 * 0x23: Chassis / IR temperature sensor. Returns a single byte in
	 * out4[0] representing degrees Celsius (measured range 34–50 °C on
	 * the OMEN Transcend 14 / 8C58).  This is the sensor the firmware's
	 * own thermal guard uses (thresholds 40 °C / 52 °C in OGH), not
	 * the CPU package die.  Source: ohman docs/research.md §2.
	 */
	HPWMI_CHASSIS_TEMP_QUERY           = 0x23,
};

enum hp_wmi_command {
	HPWMI_READ  = 0x01,
	HPWMI_WRITE = 0x02,
	HPWMI_ODM   = 0x03,
	HPWMI_GM    = 0x20008,
};

enum hp_wmi_hardware_mask {
	HPWMI_DOCK_MASK   = 0x01,
	HPWMI_TABLET_MASK = 0x04,
};

struct bios_return {
	u32 sigpass;
	u32 return_code;
};

enum hp_return_value {
	HPWMI_RET_WRONG_SIGNATURE   = 0x02,
	HPWMI_RET_UNKNOWN_COMMAND   = 0x03,
	HPWMI_RET_UNKNOWN_CMDTYPE   = 0x04,
	HPWMI_RET_INVALID_PARAMETERS = 0x05,
};

enum hp_wireless2_bits {
	HPWMI_POWER_STATE    = 0x01,
	HPWMI_POWER_SOFT     = 0x02,
	HPWMI_POWER_BIOS     = 0x04,
	HPWMI_POWER_HARD     = 0x08,
	HPWMI_POWER_FW_OR_HW = HPWMI_POWER_BIOS | HPWMI_POWER_HARD,
};

#define IS_HWBLOCKED(x) (((x) & HPWMI_POWER_FW_OR_HW) != HPWMI_POWER_FW_OR_HW)
#define IS_SWBLOCKED(x) (!((x) & HPWMI_POWER_SOFT))

struct bios_rfkill2_device_state {
	u8  radio_type;
	u8  bus_type;
	u16 vendor_id;
	u16 product_id;
	u16 subsys_vendor_id;
	u16 subsys_product_id;
	u8  rfkill_id;
	u8  power;
	u8  unknown[4];
};

/* 7 devices fit into the 128-byte buffer */
#define HPWMI_MAX_RFKILL2_DEVICES 7

struct bios_rfkill2_state {
	u8 unknown[7];
	u8 count;
	u8 pad[8];
	struct bios_rfkill2_device_state device[HPWMI_MAX_RFKILL2_DEVICES];
};

static const struct key_entry hp_wmi_keymap[] = {
	{KE_KEY,    0x02,    {KEY_BRIGHTNESSUP}},
	{KE_KEY,    0x03,    {KEY_BRIGHTNESSDOWN}},
	{KE_KEY,    0x270,   {KEY_MICMUTE}},
	{KE_KEY,    0x20e6,  {KEY_PROG1}},
	{KE_KEY,    0x20e8,  {KEY_MEDIA}},
	{KE_KEY,    0x2142,  {KEY_MEDIA}},
	{KE_KEY,    0x213b,  {KEY_INFO}},
	{KE_KEY,    0x2169,  {KEY_ROTATE_DISPLAY}},
	{KE_KEY,    0x216a,  {KEY_SETUP}},
	{KE_IGNORE, 0x21a4,  {}},             /* Win Lock On  */
	{KE_IGNORE, 0x121a4, {}},             /* Win Lock Off */
	{KE_KEY,    0x21a5,  {KEY_PROG2}},    /* HP Omen Key */
	{KE_KEY,    0x21a7,  {KEY_FN_ESC}},
	{KE_KEY,    0x21a8,  {KEY_PROG2}},    /* HP Envy x360 programmable key */
	{KE_KEY,    0x21a9,  {KEY_TOUCHPAD_OFF}},             /* Touchpad Off */
	{KE_KEY,    0x121a9, {KEY_TOUCHPAD_ON}},              /* Touchpad On  */
	{KE_KEY,    0x231b,  {KEY_HELP}},
	{KE_IGNORE, 0x21ab,  {}},             /* FnLock on */
	{KE_IGNORE, 0x121ab, {}},             /* FnLock off */
	{KE_IGNORE, 0x30021aa, {}},           /* kbd backlight: level 2 -> off */
	{KE_IGNORE, 0x33221aa, {}},           /* kbd backlight: off -> level 1 */
	{KE_IGNORE, 0x36421aa, {}},           /* kbd backlight: level 1 -> level 2 */
	{KE_END,    0}
};

/*
 * Mutex for the active_platform_profile variable,
 * see omen_powersource_event.
 */
static DEFINE_MUTEX(active_platform_profile_lock);

static struct input_dev    *hp_wmi_input_dev;
static struct input_dev    *camera_shutter_input_dev;
static struct platform_device *hp_wmi_platform_dev;
static struct device       *platform_profile_device;
static struct notifier_block platform_power_source_nb;
static enum platform_profile_option active_platform_profile;
static bool platform_profile_support;
static bool zero_insize_support;

static bool force_fan_control_support = true;
module_param(force_fan_control_support, bool, 0444);
MODULE_PARM_DESC(force_fan_control_support,
		 "Force support for manual fan control features (default: true)");

static struct rfkill *wifi_rfkill;
static struct rfkill *bluetooth_rfkill;
static struct rfkill *wwan_rfkill;

struct rfkill2_device {
	u8            id;
	int           num;
	struct rfkill *rfkill;
};

static int rfkill2_count;
static struct rfkill2_device rfkill2[HPWMI_MAX_RFKILL2_DEVICES];

/*
 * Chassis Types values were obtained from SMBIOS reference specification
 * version 3.00. A complete list of system enclosures and chassis types is
 * available in Table 17.
 */
static const char *const tablet_chassis_types[] = {
	"30", /* Tablet      */
	"31", /* Convertible */
	"32"  /* Detachable  */
};

#define DEVICE_MODE_TABLET 0x06

#define CPU_FAN 0
#define GPU_FAN 1
#define VICTUS_S_FALLBACK_MAX_RPM_FW 60
#define VICTUS_S_FALLBACK_MAX_RPM    (VICTUS_S_FALLBACK_MAX_RPM_FW * 100)

enum pwm_modes {
	PWM_MODE_MAX    = 0,
	PWM_MODE_MANUAL = 1,
	PWM_MODE_AUTO   = 2,
};

struct hp_wmi_hwmon_priv {
	u16 target_rpms[2];
	u16 max_rpms[2];
	u8  min_rpm;
	u8  max_rpm;
	u8  gpu_delta;
	u8  mode;
	u8  prev_mode;
	u8  pwm;
	bool fan_speed_available; /* true only when fan table query succeeded */
	bool uses_victus_s_fan_commands;
	struct delayed_work keep_alive_dwork;
	/*
	 * Protects mode, prev_mode, target_rpms, pwm, and serialises
	 * calls to hp_wmi_apply_fan_settings().
	 *
	 * Locking rules
	 * -------------
	 * - hwmon_read / hwmon_write: acquire before accessing any priv field
	 *   or calling apply_fan_settings.
	 * - keep_alive_handler: acquire at entry; return early if mode is AUTO
	 *   so that apply_fan_settings (which calls cancel_delayed_work, the
	 *   non-_sync variant) is never reached from within the handler while
	 *   holding the lock — avoiding a self-deadlock.
	 * - PWM_MODE_AUTO transition from hwmon_write: release the lock,
	 *   call cancel_delayed_work_sync to guarantee the handler has exited,
	 *   then re-acquire before updating state. A comment marks this window.
	 */
	struct mutex hwmon_lock;
};

struct victus_s_fan_table_header {
	u8 num_fans;
	u8 unknown;
} __packed;

struct victus_s_fan_table_entry {
	u8 cpu_rpm;
	u8 gpu_rpm;
	u8 noise_db;
} __packed;

struct victus_s_fan_table {
	struct victus_s_fan_table_header header;
	struct victus_s_fan_table_entry  entries[];
} __packed;

/*
 * 90 s delay to prevent the firmware from resetting fan mode after its
 * fixed 120 s timeout.
 */
#define KEEP_ALIVE_DELAY_SECS 90

static inline u8 rpm_to_pwm(u8 rpm, struct hp_wmi_hwmon_priv *priv)
{
	return fixp_linear_interpolate(0, 0, priv->max_rpm, U8_MAX,
				       clamp_val(rpm, 0, priv->max_rpm));
}

static inline u8 pwm_to_rpm(u8 pwm, struct hp_wmi_hwmon_priv *priv)
{
	return fixp_linear_interpolate(0, 0, U8_MAX, priv->max_rpm,
				       clamp_val(pwm, 0, U8_MAX));
}

/* Map output size to the corresponding WMI method id */
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

/*
 * hp_wmi_perform_query
 *
 * query:   The commandtype (enum hp_wmi_commandtype)
 * command: The command     (enum hp_wmi_command)
 * buffer:  Buffer used as input and/or output
 * insize:  Size of input buffer
 * outsize: Size of output buffer
 *
 * Returns zero on success, a positive HP WMI error code, or a negative
 * errno.  The buffersize must be at least max(insize, outsize).
 */
/* Global mutex to prevent concurrent WMI evaluation across modules (e.g. RGB driver) */
DEFINE_MUTEX(hp_wmi_mutex);
EXPORT_SYMBOL_GPL(hp_wmi_mutex);

static int hp_wmi_perform_query(int query, enum hp_wmi_command command,
                            void *buffer, int insize, int outsize)
{
        struct acpi_buffer input, output = {ACPI_ALLOCATE_BUFFER, NULL};
        struct bios_return *bios_return;
        union acpi_object *obj = NULL;
        struct bios_args *args = NULL;
        int mid, actual_insize, actual_outsize;
        size_t bios_args_size;
        int ret;
        bool retried = false;

        mid = encode_outsize_for_pvsz(outsize);
        if (WARN_ON(mid < 0))
                return mid;

        actual_insize = insize;
        bios_args_size = struct_size(args, data, actual_insize);
        bios_args_size = max_t(size_t, bios_args_size, 128);
        args = kzalloc(bios_args_size, GFP_KERNEL);
        if (!args)
                return -ENOMEM;

        input.length  = bios_args_size;
        input.pointer = args;

        args->signature   = 0x55434553;
        args->command     = command;
        args->commandtype = query;
        args->datasize = (insize == 0 && !zero_insize_support) ? 4 : insize;

        if (insize > 0)
                memcpy(args->data, buffer, flex_array_size(args, data, insize));

retry:
        mutex_lock(&hp_wmi_mutex);
        ret = wmi_evaluate_method(retried ? HPWMI_BIOS_GUID : active_bios_guid, 0, mid, &input, &output);
        mutex_unlock(&hp_wmi_mutex);

        if (ret)
                goto check_retry;

        obj = output.pointer;
        if (!obj) {
                ret = -EINVAL;
                goto check_retry;
        }

        if (obj->type != ACPI_TYPE_BUFFER) {
                ret = -EINVAL;
                goto check_retry;
        }

        if (!obj->buffer.pointer || obj->buffer.length < sizeof(*bios_return)) {
                ret = -EINVAL;
                goto check_retry;
        }

        bios_return = (struct bios_return *)obj->buffer.pointer;
        ret = bios_return->return_code;

check_retry:
        if (ret) {
                if (!retried && !strcmp(active_bios_guid, HPWMI_OMEN_HPC_GUID)) {
                        kfree(output.pointer);
                        output.pointer = NULL;
                        output.length = ACPI_ALLOCATE_BUFFER;
                        retried = true;
                        goto retry;
                }

                if (obj) {
                        if (obj->type != ACPI_TYPE_BUFFER) {
                                pr_warn("query 0x%x returned an invalid object type 0x%x\n", query, obj->type);
                        } else if (!obj->buffer.pointer || obj->buffer.length < sizeof(*bios_return)) {
                                pr_warn("query 0x%x returned invalid buffer (ptr=%p len=%u)\n", query, obj->buffer.pointer, obj->buffer.length);
                        } else if (ret != HPWMI_RET_UNKNOWN_COMMAND && ret != HPWMI_RET_UNKNOWN_CMDTYPE) {
                                pr_warn("query 0x%x returned error 0x%x\n", query, ret);
                        }
                }
                goto out_free;
        }

        if (!outsize)
                goto out_free;

        actual_outsize = min(outsize,
                             (int)(obj->buffer.length - sizeof(*bios_return)));
        memcpy(buffer, obj->buffer.pointer + sizeof(*bios_return),
               actual_outsize);
        memset(buffer + actual_outsize, 0, outsize - actual_outsize);

out_free:
        kfree(output.pointer);
        kfree(args);
        return ret;
}

/*
 * Calling hp_wmi_get_fan_count_userdefine_trigger() also enables and/or
 * maintains the laptop in user-defined thermal and fan states, instead of
 * using a fallback state.  After a 120 s timeout the laptop reverts to its
 * fallback state.
 */
static int hp_wmi_get_fan_count_userdefine_trigger(void)
{
	u8 fan_data[4] = {};
	int ret;

	ret = hp_wmi_perform_query(HPWMI_FAN_COUNT_GET_QUERY, HPWMI_GM,
				   &fan_data, sizeof(u8), sizeof(fan_data));
	if (ret != 0)
		return -EINVAL;

	return fan_data[0]; /* Other bytes do not carry the fan count */
}

static int hp_wmi_get_fan_speed(int fan)
{
	char fan_data[4] = {fan, 0, 0, 0};
	int ret;
	u8 fsh, fsl;

	ret = hp_wmi_perform_query(HPWMI_FAN_SPEED_GET_QUERY, HPWMI_GM,
				   &fan_data, sizeof(char), sizeof(fan_data));
	if (ret != 0)
		return -EINVAL;

	fsh = fan_data[2];
	fsl = fan_data[3];

	return (fsh << 8) | fsl;
}

static int hp_wmi_get_fan_speed_victus_s(int fan)
{
	u8 fan_data[128] = {};
	int ret;

	if (fan < 0 || fan > 1)
		return -EINVAL;

	ret = hp_wmi_perform_query(HPWMI_VICTUS_S_FAN_SPEED_GET_QUERY,
				   HPWMI_GM, &fan_data, sizeof(u8),
				   sizeof(fan_data));
	if (ret != 0)
		return -EINVAL;

	return fan_data[fan] * 100;
}

static int hp_wmi_read_int(int query)
{
	int val = 0, ret;

	ret = hp_wmi_perform_query(query, HPWMI_READ, &val,
				   zero_if_sup(val), sizeof(val));
	if (ret)
		return ret < 0 ? ret : -EINVAL;

	return val;
}

static int hp_wmi_get_dock_state(void)
{
	int state = hp_wmi_read_int(HPWMI_HARDWARE_QUERY);

	if (state < 0)
		return state;

	return !!(state & HPWMI_DOCK_MASK);
}

static int hp_wmi_get_tablet_mode(void)
{
	char system_device_mode[4] = {0};
	const char *chassis_type;
	bool tablet_found;
	int ret;

	chassis_type = dmi_get_system_info(DMI_CHASSIS_TYPE);
	if (!chassis_type)
		return -ENODEV;

	tablet_found = match_string(tablet_chassis_types,
				    ARRAY_SIZE(tablet_chassis_types),
				    chassis_type) >= 0;
	if (!tablet_found)
		return -ENODEV;

	ret = hp_wmi_perform_query(HPWMI_SYSTEM_DEVICE_MODE, HPWMI_READ,
				   system_device_mode,
				   zero_if_sup(system_device_mode),
				   sizeof(system_device_mode));
	if (ret < 0)
		return ret;

	return system_device_mode[0] == DEVICE_MODE_TABLET;
}

static int omen_thermal_profile_set(int mode)
{
	/*
	 * The Omen Control Center actively sets the first byte of the buffer
	 * to 0xFF, so mimic that behaviour.
	 */
	u8 buffer[2] = {0xFF, mode};
	int ret;

	ret = hp_wmi_perform_query(HPWMI_SET_PERFORMANCE_MODE, HPWMI_GM,
				   &buffer, sizeof(buffer), 0);
	if (ret)
		return ret < 0 ? ret : -EINVAL;

	return mode;
}

static bool is_omen_thermal_profile(void)
{
	const char *board_name = dmi_get_system_info(DMI_BOARD_NAME);

	if (!board_name)
		return false;

	return match_string(omen_thermal_profile_boards,
			    ARRAY_SIZE(omen_thermal_profile_boards),
			    board_name) >= 0;
}

static int omen_get_thermal_policy_version(void)
{
	unsigned char buffer[8] = {0};
	const char *board_name;
	int ret;

	board_name = dmi_get_system_info(DMI_BOARD_NAME);
	if (board_name) {
		int matches = match_string(
			omen_thermal_profile_force_v0_boards,
			ARRAY_SIZE(omen_thermal_profile_force_v0_boards),
			board_name);
		if (matches >= 0)
			return 0;
	}

	ret = hp_wmi_perform_query(HPWMI_GET_SYSTEM_DESIGN_DATA, HPWMI_GM,
				   &buffer, sizeof(buffer), sizeof(buffer));
	if (ret)
		return ret < 0 ? ret : -EINVAL;

	return buffer[3];
}

static int omen_thermal_profile_get(void)
{
	u8 offset = HP_OMEN_EC_THERMAL_PROFILE_OFFSET;
	u8 data;
	int ret;

	if (active_thermal_profile_params &&
	    active_thermal_profile_params->ec_tp_offset != HP_EC_OFFSET_UNKNOWN &&
	    active_thermal_profile_params->ec_tp_offset != HP_NO_THERMAL_PROFILE_OFFSET)
		offset = active_thermal_profile_params->ec_tp_offset;

	ret = ec_read(offset, &data);
	if (ret)
		return ret;

	return data;
}

static int hp_wmi_fan_speed_max_set(int enabled)
{
	int ret;

	ret = hp_wmi_perform_query(HPWMI_FAN_SPEED_MAX_SET_QUERY, HPWMI_GM,
				   &enabled, sizeof(enabled), 0);
	if (ret)
		return ret < 0 ? ret : -EINVAL;

	return enabled;
}

static int hp_wmi_fan_speed_set(struct hp_wmi_hwmon_priv *priv,
				u16 cpu_rpm, u16 gpu_rpm)
{
	u8 fan_speed[2];
	int ret;

	/* BIOS expects values in units of 100 RPM */
	fan_speed[CPU_FAN] = (u8)(cpu_rpm / 100);
	fan_speed[GPU_FAN] = (u8)(gpu_rpm / 100);

	ret = hp_wmi_get_fan_count_userdefine_trigger();
	if (ret < 0)
		return ret;

	/* Max-fan mode must be explicitly disabled before setting a speed */
	ret = hp_wmi_fan_speed_max_set(0);
	if (ret < 0)
		return ret;

	return hp_wmi_perform_query(HPWMI_VICTUS_S_FAN_SPEED_SET_QUERY,
				    HPWMI_GM, &fan_speed, sizeof(fan_speed), 0);
}

static bool is_victus_thermal_profile(void);
static bool is_victus_s_thermal_profile(void);
static int platform_profile_omen_set_ec(enum platform_profile_option profile);
static int platform_profile_victus_set_ec(enum platform_profile_option profile);
static int platform_profile_victus_s_set_ec(enum platform_profile_option profile);
static int thermal_profile_get(void);
static int thermal_profile_set(int thermal_profile);

/*
 * hp_wmi_fan_speed_reset - hand fan control back to the EC.
 *
 * Disables max-fan mode, then re-applies the active platform profile so the EC
 * instantly resumes automatic control without waiting for the watchdog timeout.
 */
static int hp_wmi_fan_speed_reset(struct hp_wmi_hwmon_priv *priv)
{
	int ret;

	ret = hp_wmi_fan_speed_max_set(0);
	if (ret)
		return ret;

	if (!priv->fan_speed_available)
		return 0;

	/*
	 * Previously, an explicit zero-RPM command (HPWMI_VICTUS_S_FAN_SPEED_SET_QUERY)
	 * was sent here. However, sending {0, 0} causes the EC to remain in manual mode
	 * at 0 RPM for up to 120 seconds until the internal watchdog times out, leaving
	 * the fans completely stopped even under heavy load.
	 *
	 * To force the EC to instantly release manual override and resume automatic
	 * fan control without waiting for the watchdog timeout, we re-apply the active
	 * thermal profile via HPWMI_SET_PERFORMANCE_MODE.
	 */
	if (is_omen_thermal_profile())
		platform_profile_omen_set_ec(active_platform_profile);
	else if (is_victus_thermal_profile())
		platform_profile_victus_set_ec(active_platform_profile);
	else if (is_victus_s_thermal_profile())
		platform_profile_victus_s_set_ec(active_platform_profile);
	else
		thermal_profile_set(thermal_profile_get());

	return 0;
}

/*
 * hp_wmi_fan_speed_max_reset - leave max-fan mode.
 *
 * On Victus S-series laptops leaving max-fan mode requires two steps:
 * clearing the max-fan flag and then sending a zero-RPM command.
 */
static int hp_wmi_fan_speed_max_reset(struct hp_wmi_hwmon_priv *priv)
{
	/* hp_wmi_fan_speed_reset already calls hp_wmi_fan_speed_max_set(0) */
	return hp_wmi_fan_speed_reset(priv);
}

static int __init hp_wmi_bios_2008_later(void)
{
	int state = 0;
	int ret = hp_wmi_perform_query(HPWMI_FEATURE_QUERY, HPWMI_READ,
				       &state, zero_if_sup(state), sizeof(state));
	if (!ret)
		return 1;

	return (ret == HPWMI_RET_UNKNOWN_CMDTYPE) ? 0 : -ENXIO;
}

static int __init hp_wmi_bios_2009_later(void)
{
	u8 state[128];
	int ret = hp_wmi_perform_query(HPWMI_FEATURE2_QUERY, HPWMI_READ,
				       &state, zero_if_sup(state), sizeof(state));
	if (!ret)
		return 1;

	return (ret == HPWMI_RET_UNKNOWN_CMDTYPE) ? 0 : -ENXIO;
}

static int __init hp_wmi_enable_hotkeys(void)
{
	int value = 0x6e;
	int ret = hp_wmi_perform_query(HPWMI_BIOS_QUERY, HPWMI_WRITE,
				       &value, sizeof(value), 0);

	return ret <= 0 ? ret : -EINVAL;
}

static int hp_wmi_set_block(void *data, bool blocked)
{
	enum hp_wmi_radio r = (long)data;
	int query = BIT(r + 8) | ((!blocked) << r);
	int ret;

	ret = hp_wmi_perform_query(HPWMI_WIRELESS_QUERY, HPWMI_WRITE,
				   &query, sizeof(query), 0);
	return ret <= 0 ? ret : -EINVAL;
}

static const struct rfkill_ops hp_wmi_rfkill_ops = {
	.set_block = hp_wmi_set_block,
};

static bool hp_wmi_get_sw_state(enum hp_wmi_radio r)
{
	int mask    = 0x200 << (r * 8);
	int wireless = hp_wmi_read_int(HPWMI_WIRELESS_QUERY);

	WARN_ONCE(wireless < 0, "error executing HPWMI_WIRELESS_QUERY");
	return !(wireless & mask);
}

static bool hp_wmi_get_hw_state(enum hp_wmi_radio r)
{
	int mask    = 0x800 << (r * 8);
	int wireless = hp_wmi_read_int(HPWMI_WIRELESS_QUERY);

	WARN_ONCE(wireless < 0, "error executing HPWMI_WIRELESS_QUERY");
	return !(wireless & mask);
}

static int hp_wmi_rfkill2_set_block(void *data, bool blocked)
{
	int rfkill_id = (int)(long)data;
	char buffer[4] = {0x01, 0x00, rfkill_id, !blocked};
	int ret;

	ret = hp_wmi_perform_query(HPWMI_WIRELESS2_QUERY, HPWMI_WRITE,
				   buffer, sizeof(buffer), 0);
	return ret <= 0 ? ret : -EINVAL;
}

static const struct rfkill_ops hp_wmi_rfkill2_ops = {
	.set_block = hp_wmi_rfkill2_set_block,
};

static int hp_wmi_rfkill2_refresh(void)
{
	struct bios_rfkill2_state state = { 0 };
	int err, i;

	err = hp_wmi_perform_query(HPWMI_WIRELESS2_QUERY, HPWMI_READ, &state,
				   zero_if_sup(state), sizeof(state));
	if (err)
		return err;

	for (i = 0; i < rfkill2_count; i++) {
		int num = rfkill2[i].num;
		struct bios_rfkill2_device_state *devstate;

		devstate = &state.device[num];

		if (num >= state.count ||
		    devstate->rfkill_id != rfkill2[i].id) {
			pr_warn("power configuration of the wireless devices unexpectedly changed\n");
			continue;
		}

		rfkill_set_states(rfkill2[i].rfkill,
				  IS_SWBLOCKED(devstate->power),
				  IS_HWBLOCKED(devstate->power));
	}

	return 0;
}

static ssize_t display_show(struct device *dev, struct device_attribute *attr,
			    char *buf)
{
	int value = hp_wmi_read_int(HPWMI_DISPLAY_QUERY);

	if (value < 0)
		return value;
	return sysfs_emit(buf, "%d\n", value);
}

static ssize_t hddtemp_show(struct device *dev, struct device_attribute *attr,
			    char *buf)
{
	int value = hp_wmi_read_int(HPWMI_HDDTEMP_QUERY);

	if (value < 0)
		return value;
	return sysfs_emit(buf, "%d\n", value);
}

static ssize_t als_show(struct device *dev, struct device_attribute *attr,
			char *buf)
{
	int value = hp_wmi_read_int(HPWMI_ALS_QUERY);

	if (value < 0)
		return value;
	return sysfs_emit(buf, "%d\n", value);
}

static ssize_t dock_show(struct device *dev, struct device_attribute *attr,
			 char *buf)
{
	int value = hp_wmi_get_dock_state();

	if (value < 0)
		return value;
	return sysfs_emit(buf, "%d\n", value);
}

static ssize_t tablet_show(struct device *dev, struct device_attribute *attr,
			   char *buf)
{
	int value = hp_wmi_get_tablet_mode();

	if (value < 0)
		return value;
	return sysfs_emit(buf, "%d\n", value);
}

static ssize_t postcode_show(struct device *dev, struct device_attribute *attr,
			     char *buf)
{
	int value = hp_wmi_read_int(HPWMI_POSTCODEERROR_QUERY);

	if (value < 0)
		return value;
	return sysfs_emit(buf, "0x%x\n", value);
}

static ssize_t als_store(struct device *dev, struct device_attribute *attr,
			 const char *buf, size_t count)
{
	u32 tmp;
	int ret;

	ret = kstrtou32(buf, 10, &tmp);
	if (ret)
		return ret;
	if (tmp > 1)
		return -EINVAL;

	ret = hp_wmi_perform_query(HPWMI_ALS_QUERY, HPWMI_WRITE,
				   &tmp, sizeof(tmp), 0);
	if (ret)
		return ret < 0 ? ret : -EINVAL;

	return count;
}

static ssize_t postcode_store(struct device *dev, struct device_attribute *attr,
			      const char *buf, size_t count)
{
	u32 tmp = 1;
	bool clear;
	int ret;

	ret = kstrtobool(buf, &clear);
	if (ret)
		return ret;

	if (!clear)
		return -EINVAL;

	ret = hp_wmi_perform_query(HPWMI_POSTCODEERROR_QUERY, HPWMI_WRITE,
				   &tmp, sizeof(tmp), 0);
	if (ret)
		return ret < 0 ? ret : -EINVAL;

	return count;
}

static bool hp_wmi_gpu_mode_supported(void)
{
	char system_device_mode[4] = {0};
	int ret;

	/*
	 * FIX: use zero_if_sup() to be consistent with all other READ calls
	 * using HPWMI_SYSTEM_DEVICE_MODE; passing a hard sizeof() on devices
	 * with zero_insize_support caused a spurious failure that made the
	 * graphics_mode sysfs attribute invisible.
	 */
	ret = hp_wmi_perform_query(HPWMI_SYSTEM_DEVICE_MODE, HPWMI_READ,
				   system_device_mode,
				   zero_if_sup(system_device_mode),
				   sizeof(system_device_mode));
	if (ret < 0)
		return false;

	/*
	 * HP uses command 0x40 for both Tablet Mode and Graphics Switch.
	 * Graphics modes are 0, 1, 2; tablet mode is 6.
	 */
	return system_device_mode[0] <= 2;
}

static int victus_s_gpu_thermal_profile_get(bool *ctgp_enable,
					    bool *ppab_enable,
					    u8 *dstate,
					    u8 *gpu_slowdown_temp);

static int victus_s_gpu_thermal_profile_set(bool ctgp_enable,
					    bool ppab_enable,
					    u8 dstate);

static bool is_victus_s_thermal_profile(void);

static ssize_t gpu_tgp_show(struct device *dev, struct device_attribute *attr,
			    char *buf)
{
	bool ctgp, ppab;
	u8 dstate, slowdown;
	int ret;

	guard(mutex)(&active_platform_profile_lock);

	ret = victus_s_gpu_thermal_profile_get(&ctgp, &ppab, &dstate, &slowdown);
	if (ret < 0)
		return ret;

	return sysfs_emit(buf, "%d\n", ctgp);
}

static ssize_t gpu_tgp_store(struct device *dev, struct device_attribute *attr,
			     const char *buf, size_t count)
{
	bool ppab, target;
	u8 dstate;
	int ret;

	ret = kstrtobool(buf, &target);
	if (ret)
		return ret;

	guard(mutex)(&active_platform_profile_lock);

	ret = victus_s_gpu_thermal_profile_get(NULL, &ppab, &dstate, NULL);
	if (ret < 0)
		return ret;

	ret = victus_s_gpu_thermal_profile_set(target, ppab, dstate);
	if (ret < 0)
		return ret;

	return count;
}

static ssize_t gpu_ppab_show(struct device *dev, struct device_attribute *attr,
			     char *buf)
{
	bool ctgp, ppab;
	u8 dstate, slowdown;
	int ret;

	guard(mutex)(&active_platform_profile_lock);

	ret = victus_s_gpu_thermal_profile_get(&ctgp, &ppab, &dstate, &slowdown);
	if (ret < 0)
		return ret;

	return sysfs_emit(buf, "%d\n", ppab);
}

static ssize_t gpu_ppab_store(struct device *dev, struct device_attribute *attr,
			      const char *buf, size_t count)
{
	bool ctgp, target;
	u8 dstate;
	int ret;

	ret = kstrtobool(buf, &target);
	if (ret)
		return ret;

	guard(mutex)(&active_platform_profile_lock);

	ret = victus_s_gpu_thermal_profile_get(&ctgp, NULL, &dstate, NULL);
	if (ret < 0)
		return ret;

	ret = victus_s_gpu_thermal_profile_set(ctgp, target, dstate);
	if (ret < 0)
		return ret;

	return count;
}

static ssize_t graphics_mode_show(struct device *dev,
				  struct device_attribute *attr, char *buf)
{
	char system_device_mode[4] = {0};
	int ret;

	ret = hp_wmi_perform_query(HPWMI_SYSTEM_DEVICE_MODE, HPWMI_READ,
				   system_device_mode,
				   zero_if_sup(system_device_mode),
				   sizeof(system_device_mode));
	if (ret < 0)
		return ret;

	return sysfs_emit(buf, "%d\n", system_device_mode[0]);
}

static ssize_t graphics_mode_store(struct device *dev,
				   struct device_attribute *attr,
				   const char *buf, size_t count)
{
	u8 mode_buf_small[1];
	u32 mode_buf_u32;
	u8 mode_buf_padded[128] = {0};
	u32 tmp;
	int ret;

	ret = kstrtou32(buf, 10, &tmp);
	if (ret)
		return ret;

	/* Only allow 0, 1, 2 for graphics; 6 (tablet) is handled separately */
	if (tmp > 2)
		return -EINVAL;

	mode_buf_small[0] = (u8)tmp;
	mode_buf_u32 = tmp;
	mode_buf_padded[0] = (u8)tmp;

	/*
	 * Firmware is inconsistent across models: some expect a compact payload
	 * (1 byte), some expect 4 bytes (u32), others require a padded buffer.
	 * Try compact first, then 4 bytes, then padded.
	 */
	ret = hp_wmi_perform_query(HPWMI_SYSTEM_DEVICE_MODE, HPWMI_WRITE,
				   mode_buf_small, sizeof(mode_buf_small), 0);
	if (ret == HPWMI_RET_INVALID_PARAMETERS || ret == -EINVAL)
		ret = hp_wmi_perform_query(HPWMI_SYSTEM_DEVICE_MODE, HPWMI_WRITE,
					   &mode_buf_u32, sizeof(mode_buf_u32), 0);
	if (ret == HPWMI_RET_INVALID_PARAMETERS || ret == -EINVAL)
		ret = hp_wmi_perform_query(HPWMI_SYSTEM_DEVICE_MODE, HPWMI_WRITE,
					   mode_buf_padded, sizeof(mode_buf_padded), 0);

	if (ret)
		return ret < 0 ? ret : -EINVAL;

	return count;
}

static int camera_shutter_input_setup(void)
{
	int err;

	camera_shutter_input_dev = input_allocate_device();
	if (!camera_shutter_input_dev)
		return -ENOMEM;

	camera_shutter_input_dev->name    = "HP WMI camera shutter";
	camera_shutter_input_dev->phys    = "wmi/input1";
	camera_shutter_input_dev->id.bustype = BUS_HOST;

	__set_bit(EV_SW,              camera_shutter_input_dev->evbit);
	__set_bit(SW_CAMERA_LENS_COVER, camera_shutter_input_dev->swbit);

	err = input_register_device(camera_shutter_input_dev);
	if (err)
		goto err_free_dev;

	return 0;

err_free_dev:
	input_free_device(camera_shutter_input_dev);
	camera_shutter_input_dev = NULL;
	return err;
}

static const u8 mux_bitmask_map[] = {
	[0] = HPWMI_MUX_MODE_HYBRID,
	[1] = HPWMI_MUX_MODE_DISCRETE,
	[2] = HPWMI_MUX_MODE_OPTIMUS,
	[3] = HPWMI_MUX_MODE_UMA,
};

static bool has_nvidia_gpu(void)
{
	struct pci_dev *pdev = NULL;
	while ((pdev = pci_get_class(PCI_CLASS_DISPLAY_VGA << 8, pdev))) {
		if (pdev->vendor == PCI_VENDOR_ID_NVIDIA) {
			pci_dev_put(pdev);
			return true;
		}
	}
	while ((pdev = pci_get_class(PCI_CLASS_DISPLAY_3D << 8, pdev))) {
		if (pdev->vendor == PCI_VENDOR_ID_NVIDIA) {
			pci_dev_put(pdev);
			return true;
		}
	}
	return false;
}

static int hp_wmi_get_mux_supported_modes(u8 *supported)
{
	u8 legacy_buffer[4] = {};
	u8 buffer[128] = {};
	u32 req_packet = 0;
	int ret;

	if (!supported)
		return -EINVAL;

	/* Try modern BIOS design data query (128-byte buffer) */
	ret = hp_wmi_perform_query(HPWMI_GET_SYSTEM_DESIGN_DATA, HPWMI_GM,
				   buffer, zero_if_sup(req_packet), sizeof(buffer));
	if (ret == 0) {
		*supported = buffer[7];
		return 0;
	}

	/*
	 * (Fallback): Legacy BIOS behavior based on Omen Gaming Hub.
	 * If the modern query is not supported, check if the MUX query endpoint
	 * responds to a read request. If it succeeds, the hardware has MUX
	 * capability but lacks the mode map, defaulting to Hybrid + Discrete.
	 *
	 * DANGER: Sending HPWMI_GRAPHICS_MUX_QUERY to unsupported models (e.g.,
	 * AMD Advantage laptops like OMEN 16-xd0xxx) will cause a severe ACPI
	 * fault and a kernel panic. Only proceed if an NVIDIA GPU is present.
	 */
	if (!has_nvidia_gpu())
		return -ENODEV;

	ret = hp_wmi_perform_query(HPWMI_GRAPHICS_MUX_QUERY, HPWMI_READ,
				   legacy_buffer, sizeof(legacy_buffer), 0);
	if (ret < 0)
		return ret;
	if (ret > 0)
		return -EINVAL;

	*supported = HPWMI_MUX_LEGACY_MASK;
	return 0;
}

static int hp_wmi_get_mux_mode(u8 *mode)
{
	u8 buffer[4] = {};
	int ret;

	if (!mode)
		return -EINVAL;

	ret = hp_wmi_perform_query(HPWMI_GRAPHICS_MUX_QUERY, HPWMI_READ,
				   buffer, sizeof(buffer), sizeof(buffer));
	if (ret < 0)
		return ret;
	if (ret > 0)
		return -EINVAL;

	/* Mask the highest bit, which might be used as a BIOS status flag */
	*mode = buffer[0] & HPWMI_MUX_MODE_MASK;
	return 0;
}

static int hp_wmi_set_mux_mode(u8 mode)
{
	u8 buffer[4] = { mode, 0x00, 0x00, 0x00 };
	int ret;

	ret = hp_wmi_perform_query(HPWMI_GRAPHICS_MUX_QUERY, HPWMI_WRITE,
				   buffer, sizeof(buffer), sizeof(buffer));
	if (ret < 0)
		return ret;
	if (ret > 0)
		return -EINVAL;

	return 0;
}

static ssize_t gpu_mux_mode_show(struct device *dev,
				 struct device_attribute *attr,
				 char *buf)
{
	u8 mode;
	int ret;

	ret = hp_wmi_get_mux_mode(&mode);
	if (ret)
		return ret;

	return sysfs_emit(buf, "%u\n", mode);
}

static ssize_t gpu_mux_mode_store(struct device *dev,
				  struct device_attribute *attr,
				  const char *buf,
				  size_t count)
{
	u32 requested;
	u8 supported;
	int ret;

	ret = kstrtou32(buf, 0, &requested);
	if (ret)
		return ret;
	if (requested >= ARRAY_SIZE(mux_bitmask_map))
		return -EINVAL;

	ret = hp_wmi_get_mux_supported_modes(&supported);
	if (ret)
		return ret;

	/* Verify if the requested mode is allowed by the hardware mask */
	if (!(supported & mux_bitmask_map[requested]))
		return -EOPNOTSUPP;

	ret = hp_wmi_set_mux_mode(requested);
	if (ret)
		return ret;

	return count;
}

static DEVICE_ATTR_RO(display);
static DEVICE_ATTR_RO(hddtemp);
static DEVICE_ATTR_RW(als);
static DEVICE_ATTR_RO(dock);
static DEVICE_ATTR_RO(tablet);
static DEVICE_ATTR_RW(postcode);
static DEVICE_ATTR_RW(graphics_mode);
static DEVICE_ATTR_RW(gpu_tgp);
static DEVICE_ATTR_RW(gpu_ppab);
static DEVICE_ATTR_RW(gpu_mux_mode);

/*
 * chassis_temp — read-only sysfs attribute exposing the WMI 0x23 chassis/IR
 * sensor in degrees Celsius.  The value is the board surface temperature read
 * by the firmware's own thermal guard (OGH thresholds: 40 °C warn, 52 °C
 * emergency fan override).  It is NOT the CPU die temperature.
 *
 * The attribute is hidden on boards where the query is known to be
 * unsupported (return code HPWMI_RET_INVALID_PARAMETERS or negative errno).
 */
static ssize_t chassis_temp_show(struct device *dev,
				  struct device_attribute *attr, char *buf)
{
	u8 out[4] = {0};
	u8 query_in[4] = {1, 0, 0, 0};
	int ret;

	ret = hp_wmi_perform_query(HPWMI_CHASSIS_TEMP_QUERY, HPWMI_GM,
				   out, sizeof(query_in), sizeof(out));
	if (ret)
		return ret < 0 ? ret : -EIO;

	return sysfs_emit(buf, "%u\n", out[0]);
}

static DEVICE_ATTR_RO(chassis_temp);

static struct attribute *hp_wmi_attrs[] = {
	&dev_attr_display.attr,
	&dev_attr_hddtemp.attr,
	&dev_attr_als.attr,
	&dev_attr_dock.attr,
	&dev_attr_tablet.attr,
	&dev_attr_postcode.attr,
	&dev_attr_graphics_mode.attr,
	&dev_attr_gpu_tgp.attr,
	&dev_attr_gpu_ppab.attr,
	&dev_attr_gpu_mux_mode.attr,
	&dev_attr_chassis_temp.attr,
	NULL,
};

static umode_t hp_wmi_attrs_is_visible(struct kobject *kobj,
				       struct attribute *attr, int n)
{
	if (attr == &dev_attr_graphics_mode.attr)
		return hp_wmi_gpu_mode_supported() ? attr->mode : 0;

	if (attr == &dev_attr_gpu_mux_mode.attr)
		return has_mux ? attr->mode : 0;

	if (attr == &dev_attr_gpu_tgp.attr || attr == &dev_attr_gpu_ppab.attr)
		return is_victus_s_thermal_profile() ? attr->mode : 0;

	/*
	 * Probe the chassis temp query once to decide visibility.
	 * A negative errno or a non-zero WMI error hides the attribute.
	 * We test with the same {1,0,0,0} payload used at read time.
	 */
	if (attr == &dev_attr_chassis_temp.attr) {
		/* OMEN_878A_CHASSIS_TEMP_GUARD: do NOT run the init-time 0x23 chassis-temp WMI
		 * probe on WMBA-abort-prone boards. It is the only remaining init-time WMI call
		 * versus the known-bootable v2.0.3 call profile.
		 */
		{
			const char *omen_board = dmi_get_system_info(DMI_BOARD_NAME);
			if (omen_board && (!strcmp(omen_board, "878A") || !strcmp(omen_board, "8BCD") ||
					   !strcmp(omen_board, "8C75") || !strcmp(omen_board, "8BAC")))
				return 0;
		}

		u8 out[4] = {0};
		u8 q[4] = {1, 0, 0, 0};
		int r = hp_wmi_perform_query(HPWMI_CHASSIS_TEMP_QUERY,
					    HPWMI_GM, out, sizeof(q),
					    sizeof(out));
		return r ? 0 : attr->mode;
	}

	return attr->mode;
}

static const struct attribute_group hp_wmi_group = {
	.attrs      = hp_wmi_attrs,
	.is_visible = hp_wmi_attrs_is_visible,
};

static const struct attribute_group *hp_wmi_groups[] = {
	&hp_wmi_group,
	NULL,
};

static void hp_wmi_notify(union acpi_object *obj, void *context)
{
	u32 event_id, event_data;
	u32 *location;
	int key_code;

	if (!obj)
		return;
	if (obj->type != ACPI_TYPE_BUFFER) {
		pr_info("Unknown response received %d\n", obj->type);
		return;
	}

	/*
	 * Depending on ACPI version the concatenation of id and event data
	 * inside the _WED function results in an 8- or 16-byte buffer.
	 */
	location = (u32 *)obj->buffer.pointer;
	if (obj->buffer.length == 8) {
		event_id   = *location;
		event_data = *(location + 1);
	} else if (obj->buffer.length == 16) {
		event_id   = *location;
		event_data = *(location + 2);
	} else {
		pr_info("Unknown buffer length %d\n", obj->buffer.length);
		return;
	}

	switch (event_id) {
	case HPWMI_DOCK_EVENT:
		if (test_bit(SW_DOCK, hp_wmi_input_dev->swbit))
			input_report_switch(hp_wmi_input_dev, SW_DOCK,
					    hp_wmi_get_dock_state());
		if (test_bit(SW_TABLET_MODE, hp_wmi_input_dev->swbit))
			input_report_switch(hp_wmi_input_dev, SW_TABLET_MODE,
					    hp_wmi_get_tablet_mode());
		input_sync(hp_wmi_input_dev);
		break;
	case HPWMI_PARK_HDD:
		break;
	case HPWMI_SMART_ADAPTER:
		break;
	case HPWMI_BEZEL_BUTTON:
		key_code = hp_wmi_read_int(HPWMI_HOTKEY_QUERY);
		if (key_code < 0)
			break;
		if (!sparse_keymap_report_event(hp_wmi_input_dev, key_code, 1, true))
			pr_info("Unknown key code - 0x%x\n", key_code);
		break;
	case HPWMI_FN_P_HOTKEY:
		platform_profile_cycle();
		break;
	case HPWMI_OMEN_KEY:
		if (event_data) /* Only true for HP Omen */
			key_code = event_data;
		else
			key_code = hp_wmi_read_int(HPWMI_HOTKEY_QUERY);
		if (!sparse_keymap_report_event(hp_wmi_input_dev, key_code, 1, true))
			pr_info("Unknown key code - 0x%x\n", key_code);
		break;
	case HPWMI_WIRELESS:
		if (rfkill2_count) {
			hp_wmi_rfkill2_refresh();
			break;
		}
		if (wifi_rfkill)
			rfkill_set_states(wifi_rfkill,
					  hp_wmi_get_sw_state(HPWMI_WIFI),
					  hp_wmi_get_hw_state(HPWMI_WIFI));
		if (bluetooth_rfkill)
			rfkill_set_states(bluetooth_rfkill,
					  hp_wmi_get_sw_state(HPWMI_BLUETOOTH),
					  hp_wmi_get_hw_state(HPWMI_BLUETOOTH));
		if (wwan_rfkill)
			rfkill_set_states(wwan_rfkill,
					  hp_wmi_get_sw_state(HPWMI_WWAN),
					  hp_wmi_get_hw_state(HPWMI_WWAN));
		break;
	case HPWMI_CPU_BATTERY_THROTTLE:
		pr_info("Unimplemented CPU throttle because of 3-cell battery event detected\n");
		break;
	case HPWMI_LOCK_SWITCH:
	case HPWMI_LID_SWITCH:
	case HPWMI_SCREEN_ROTATION:
	case HPWMI_COOLSENSE_SYSTEM_MOBILE:
	case HPWMI_COOLSENSE_SYSTEM_HOT:
	case HPWMI_PROXIMITY_SENSOR:
	case HPWMI_BACKLIT_KB_BRIGHTNESS:
	case HPWMI_PEAKSHIFT_PERIOD:
	case HPWMI_BATTERY_CHARGE_PERIOD:
	case HPWMI_SANITIZATION_MODE:
		break;
	case HPWMI_CAMERA_TOGGLE:
		if (!camera_shutter_input_dev &&
		    camera_shutter_input_setup()) {
			pr_err("Failed to setup camera shutter input device\n");
			break;
		}
		if (event_data == 0xff)
			input_report_switch(camera_shutter_input_dev,
					    SW_CAMERA_LENS_COVER, 1);
		else if (event_data == 0xfe)
			input_report_switch(camera_shutter_input_dev,
					    SW_CAMERA_LENS_COVER, 0);
		else
			pr_warn("Unknown camera shutter state - 0x%x\n",
				event_data);
		input_sync(camera_shutter_input_dev);
		break;
	case HPWMI_SMART_EXPERIENCE_APP:
		break;
	default:
		pr_info("Unknown event_id - %d - 0x%x\n", event_id, event_data);
		break;
	}
}

static int __init hp_wmi_input_setup(void)
{
	acpi_status status;
	int err, val;

	hp_wmi_input_dev = input_allocate_device();
	if (!hp_wmi_input_dev)
		return -ENOMEM;

	hp_wmi_input_dev->name       = "HP WMI hotkeys";
	hp_wmi_input_dev->phys       = "wmi/input0";
	hp_wmi_input_dev->id.bustype = BUS_HOST;

	__set_bit(EV_SW, hp_wmi_input_dev->evbit);

	val = hp_wmi_get_dock_state();
	if (val >= 0) {
		__set_bit(SW_DOCK, hp_wmi_input_dev->swbit);
		input_report_switch(hp_wmi_input_dev, SW_DOCK, val);
	}

	val = hp_wmi_get_tablet_mode();
	if (val >= 0) {
		__set_bit(SW_TABLET_MODE, hp_wmi_input_dev->swbit);
		input_report_switch(hp_wmi_input_dev, SW_TABLET_MODE, val);
	}

	err = sparse_keymap_setup(hp_wmi_input_dev, hp_wmi_keymap, NULL);
	if (err)
		goto err_free_dev;

	input_sync(hp_wmi_input_dev);

	if (!hp_wmi_bios_2009_later() && hp_wmi_bios_2008_later())
		hp_wmi_enable_hotkeys();

	status = wmi_install_notify_handler(HPWMI_EVENT_GUID, hp_wmi_notify,
					    NULL);
	if (ACPI_FAILURE(status)) {
		err = -EIO;
		goto err_free_dev;
	}

	err = input_register_device(hp_wmi_input_dev);
	if (err)
		goto err_uninstall_notifier;

	return 0;

err_uninstall_notifier:
	wmi_remove_notify_handler(HPWMI_EVENT_GUID);
err_free_dev:
	input_free_device(hp_wmi_input_dev);
	return err;
}

static void hp_wmi_input_destroy(void)
{
	wmi_remove_notify_handler(HPWMI_EVENT_GUID);
	input_unregister_device(hp_wmi_input_dev);
}

static int __init hp_wmi_rfkill_setup(struct platform_device *device)
{
	int err, wireless;

	wireless = hp_wmi_read_int(HPWMI_WIRELESS_QUERY);
	if (wireless < 0)
		return wireless;

	err = hp_wmi_perform_query(HPWMI_WIRELESS_QUERY, HPWMI_WRITE,
				   &wireless, sizeof(wireless), 0);
	if (err)
		return err;

	if (wireless & 0x1) {
		wifi_rfkill = rfkill_alloc("hp-wifi", &device->dev,
					   RFKILL_TYPE_WLAN,
					   &hp_wmi_rfkill_ops,
					   (void *)HPWMI_WIFI);
		if (!wifi_rfkill)
			return -ENOMEM;
		rfkill_init_sw_state(wifi_rfkill,
				     hp_wmi_get_sw_state(HPWMI_WIFI));
		rfkill_set_hw_state(wifi_rfkill,
				    hp_wmi_get_hw_state(HPWMI_WIFI));
		err = rfkill_register(wifi_rfkill);
		if (err)
			goto register_wifi_error;
	}

	if (wireless & 0x2) {
		bluetooth_rfkill = rfkill_alloc("hp-bluetooth", &device->dev,
						RFKILL_TYPE_BLUETOOTH,
						&hp_wmi_rfkill_ops,
						(void *)HPWMI_BLUETOOTH);
		if (!bluetooth_rfkill) {
			err = -ENOMEM;
			goto register_bluetooth_error;
		}
		rfkill_init_sw_state(bluetooth_rfkill,
				     hp_wmi_get_sw_state(HPWMI_BLUETOOTH));
		rfkill_set_hw_state(bluetooth_rfkill,
				    hp_wmi_get_hw_state(HPWMI_BLUETOOTH));
		err = rfkill_register(bluetooth_rfkill);
		if (err)
			goto register_bluetooth_error;
	}

	if (wireless & 0x4) {
		wwan_rfkill = rfkill_alloc("hp-wwan", &device->dev,
					   RFKILL_TYPE_WWAN,
					   &hp_wmi_rfkill_ops,
					   (void *)HPWMI_WWAN);
		if (!wwan_rfkill) {
			err = -ENOMEM;
			goto register_wwan_error;
		}
		rfkill_init_sw_state(wwan_rfkill,
				     hp_wmi_get_sw_state(HPWMI_WWAN));
		rfkill_set_hw_state(wwan_rfkill,
				    hp_wmi_get_hw_state(HPWMI_WWAN));
		err = rfkill_register(wwan_rfkill);
		if (err)
			goto register_wwan_error;
	}

	return 0;

register_wwan_error:
	rfkill_destroy(wwan_rfkill);
	wwan_rfkill = NULL;
	if (bluetooth_rfkill)
		rfkill_unregister(bluetooth_rfkill);
register_bluetooth_error:
	rfkill_destroy(bluetooth_rfkill);
	bluetooth_rfkill = NULL;
	if (wifi_rfkill)
		rfkill_unregister(wifi_rfkill);
register_wifi_error:
	rfkill_destroy(wifi_rfkill);
	wifi_rfkill = NULL;
	return err;
}

static int __init hp_wmi_rfkill2_setup(struct platform_device *device)
{
	struct bios_rfkill2_state state = { 0 };
	int err, i;

	err = hp_wmi_perform_query(HPWMI_WIRELESS2_QUERY, HPWMI_READ, &state,
				   zero_if_sup(state), sizeof(state));
	if (err)
		return err < 0 ? err : -EINVAL;

	if (state.count > HPWMI_MAX_RFKILL2_DEVICES) {
		pr_warn("unable to parse 0x1b query output\n");
		return -EINVAL;
	}

	for (i = 0; i < state.count; i++) {
		struct rfkill *rfkill;
		enum rfkill_type type;
		char *name;

		switch (state.device[i].radio_type) {
		case HPWMI_WIFI:
			type = RFKILL_TYPE_WLAN;
			name = "hp-wifi";
			break;
		case HPWMI_BLUETOOTH:
			type = RFKILL_TYPE_BLUETOOTH;
			name = "hp-bluetooth";
			break;
		case HPWMI_WWAN:
			type = RFKILL_TYPE_WWAN;
			name = "hp-wwan";
			break;
		case HPWMI_GPS:
			type = RFKILL_TYPE_GPS;
			name = "hp-gps";
			break;
		default:
			pr_warn("unknown device type 0x%x\n",
				state.device[i].radio_type);
			continue;
		}

		if (!state.device[i].vendor_id) {
			pr_warn("zero device %d while %d reported\n",
				i, state.count);
			continue;
		}

		rfkill = rfkill_alloc(name, &device->dev, type,
				      &hp_wmi_rfkill2_ops, (void *)(long)i);
		if (!rfkill) {
			err = -ENOMEM;
			goto fail;
		}

		rfkill2[rfkill2_count].id     = state.device[i].rfkill_id;
		rfkill2[rfkill2_count].num    = i;
		rfkill2[rfkill2_count].rfkill = rfkill;

		rfkill_init_sw_state(rfkill, IS_SWBLOCKED(state.device[i].power));
		rfkill_set_hw_state(rfkill,  IS_HWBLOCKED(state.device[i].power));

		if (!(state.device[i].power & HPWMI_POWER_BIOS))
			pr_info("device %s blocked by BIOS\n", name);

		err = rfkill_register(rfkill);
		if (err) {
			rfkill_destroy(rfkill);
			goto fail;
		}

		rfkill2_count++;
	}

	return 0;
fail:
	for (; rfkill2_count > 0; rfkill2_count--) {
		rfkill_unregister(rfkill2[rfkill2_count - 1].rfkill);
		rfkill_destroy(rfkill2[rfkill2_count - 1].rfkill);
	}
	return err;
}

static int platform_profile_omen_get_ec(enum platform_profile_option *profile)
{
	int tp;

	tp = omen_thermal_profile_get();
	if (tp < 0)
		return tp;

	switch (tp) {
	case HP_OMEN_V0_THERMAL_PROFILE_PERFORMANCE:
	case HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE:
		*profile = PLATFORM_PROFILE_PERFORMANCE;
		break;
	case HP_OMEN_V0_THERMAL_PROFILE_DEFAULT:
	case HP_OMEN_V1_THERMAL_PROFILE_DEFAULT:
		*profile = PLATFORM_PROFILE_BALANCED;
		break;
	case HP_OMEN_V0_THERMAL_PROFILE_COOL:
	case HP_OMEN_V1_THERMAL_PROFILE_COOL:
		*profile = PLATFORM_PROFILE_COOL;
		break;
	default:
		return -EINVAL;
	}

	return 0;
}

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
#define PLATFORM_PROFILE_DEV_ARG struct device *dev
#define PLATFORM_PROFILE_DEV_PASS dev
#else
#define PLATFORM_PROFILE_DEV_ARG struct platform_profile_handler *pprof
#define PLATFORM_PROFILE_DEV_PASS pprof
#endif

static int platform_profile_omen_get(PLATFORM_PROFILE_DEV_ARG,
				     enum platform_profile_option *profile)
{
	/*
	 * Return the stored profile rather than querying the EC directly.
	 * The EC will silently reject a switch to PERFORMANCE when the laptop
	 * is on battery, which would make the profile appear to "snap back"
	 * to BALANCED immediately — contrary to the platform-profile ABI.
	 * See also omen_powersource_event().
	 */
	guard(mutex)(&active_platform_profile_lock);
	*profile = active_platform_profile;
	return 0;
}

static bool has_omen_thermal_profile_ec_timer(void)
{
	const char *board_name = dmi_get_system_info(DMI_BOARD_NAME);

	if (!board_name)
		return false;

	return match_string(omen_timed_thermal_profile_boards,
			    ARRAY_SIZE(omen_timed_thermal_profile_boards),
			    board_name) >= 0;
}

/* FIX: was missing 'static', causing external linkage and potential linker conflicts */
static inline int omen_thermal_profile_ec_flags_set(
	enum hp_thermal_profile_omen_flags flags)
{
	return ec_write(HP_OMEN_EC_THERMAL_PROFILE_FLAGS_OFFSET, flags);
}

/* FIX: was missing 'static', causing external linkage and potential linker conflicts */
static inline int omen_thermal_profile_ec_timer_set(u8 value)
{
	return ec_write(HP_OMEN_EC_THERMAL_PROFILE_TIMER_OFFSET, value);
}

static int platform_profile_omen_set_ec(enum platform_profile_option profile)
{
	enum hp_thermal_profile_omen_flags flags = 0;
	bool gpu_ctgp_enable = false;
	bool gpu_ppab_enable = false;
	u8 gpu_dstate = 1;
	int err, tp, tp_version;

	tp_version = omen_get_thermal_policy_version();
	if (tp_version < 0 || tp_version > 1)
		return -EOPNOTSUPP;

	switch (profile) {
	case PLATFORM_PROFILE_PERFORMANCE:
		tp = (tp_version == 0) ? HP_OMEN_V0_THERMAL_PROFILE_PERFORMANCE
				       : HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE;
		break;
	case PLATFORM_PROFILE_BALANCED:
		tp = (tp_version == 0) ? HP_OMEN_V0_THERMAL_PROFILE_DEFAULT
				       : HP_OMEN_V1_THERMAL_PROFILE_DEFAULT;
		break;
	case PLATFORM_PROFILE_COOL:
		tp = (tp_version == 0) ? HP_OMEN_V0_THERMAL_PROFILE_COOL
				       : HP_OMEN_V1_THERMAL_PROFILE_COOL;
		break;
	default:
		return -EOPNOTSUPP;
	}

	err = omen_thermal_profile_set(tp);
	if (err < 0)
		return err;

	if (has_omen_thermal_profile_ec_timer()) {
		err = omen_thermal_profile_ec_timer_set(0);
		if (err < 0)
			return err;
	}

	if (profile == PLATFORM_PROFILE_PERFORMANCE)
		flags = HP_OMEN_EC_FLAGS_NOTIMER | HP_OMEN_EC_FLAGS_TURBO;

	err = omen_thermal_profile_ec_flags_set(flags);
	if (err < 0)
		pr_warn("Failed to set thermal profile EC flags: %d\n", err);

	/*
	 * Modern Omen boards that also appear in victus_s_thermal_profile_boards
	 * support GPU power management (cTGP/PPAB) via the same WMI interface.
	 * Apply GPU power settings based on the selected profile so that the
	 * GPU is not stuck at its base TGP (e.g. 80 W on HP Omen Max 8D41).
	 */
	if (is_victus_s_board) {
		switch (profile) {
		case PLATFORM_PROFILE_PERFORMANCE:
			gpu_ctgp_enable = true;
			gpu_ppab_enable = true;
			break;
		case PLATFORM_PROFILE_BALANCED:
			gpu_ppab_enable = true;
			break;
		default:
			break;
		}

		/*
		 * The fan-count query (WMI 0x10) doubles as a "user-defined
		 * mode" trigger.  The firmware requires this before it will
		 * accept GPU thermal mode changes (WMI 0x22).  This mirrors
		 * the call in platform_profile_victus_s_set_ec().
		 */
		hp_wmi_get_fan_count_userdefine_trigger();

		err = victus_s_gpu_thermal_profile_set(gpu_ctgp_enable,
						       gpu_ppab_enable,
						       gpu_dstate);
		if (err < 0)
			pr_warn("Failed to set GPU power modes: %d\n", err);
	}

	return 0;
}

static int platform_profile_omen_set(PLATFORM_PROFILE_DEV_ARG,
				     enum platform_profile_option profile)
{
	int err;

	guard(mutex)(&active_platform_profile_lock);

	err = platform_profile_omen_set_ec(profile);
	if (err < 0)
		return err;

	active_platform_profile = profile;
	return 0;
}

static int thermal_profile_get(void)
{
	return hp_wmi_read_int(HPWMI_THERMAL_PROFILE_QUERY);
}

static int thermal_profile_set(int thermal_profile)
{
	return hp_wmi_perform_query(HPWMI_THERMAL_PROFILE_QUERY, HPWMI_WRITE,
				    &thermal_profile, sizeof(thermal_profile), 0);
}

static int hp_wmi_platform_profile_get(PLATFORM_PROFILE_DEV_ARG,
				       enum platform_profile_option *profile)
{
	int tp;

	tp = thermal_profile_get();
	if (tp < 0)
		return tp;

	switch (tp) {
	case HP_THERMAL_PROFILE_PERFORMANCE:
		*profile = PLATFORM_PROFILE_PERFORMANCE;
		break;
	case HP_THERMAL_PROFILE_DEFAULT:
		*profile = PLATFORM_PROFILE_BALANCED;
		break;
	case HP_THERMAL_PROFILE_COOL:
		*profile = PLATFORM_PROFILE_COOL;
		break;
	case HP_THERMAL_PROFILE_QUIET:
		*profile = PLATFORM_PROFILE_QUIET;
		break;
	default:
		return -EINVAL;
	}

	return 0;
}

static int hp_wmi_platform_profile_set(PLATFORM_PROFILE_DEV_ARG,
				       enum platform_profile_option profile)
{
	int err, tp;

	switch (profile) {
	case PLATFORM_PROFILE_PERFORMANCE:
		tp = HP_THERMAL_PROFILE_PERFORMANCE;
		break;
	case PLATFORM_PROFILE_BALANCED:
		tp = HP_THERMAL_PROFILE_DEFAULT;
		break;
	case PLATFORM_PROFILE_COOL:
		tp = HP_THERMAL_PROFILE_COOL;
		break;
	case PLATFORM_PROFILE_QUIET:
		tp = HP_THERMAL_PROFILE_QUIET;
		break;
	default:
		return -EOPNOTSUPP;
	}

	err = thermal_profile_set(tp);
	return err;
}

static bool is_victus_thermal_profile(void)
{
	const char *board_name = dmi_get_system_info(DMI_BOARD_NAME);

	if (!board_name)
		return false;

	return match_string(victus_thermal_profile_boards,
			    ARRAY_SIZE(victus_thermal_profile_boards),
			    board_name) >= 0;
}

static int platform_profile_victus_get_ec(enum platform_profile_option *profile)
{
	int tp;

	tp = omen_thermal_profile_get();
	if (tp < 0)
		return tp;

	switch (tp) {
	case HP_VICTUS_THERMAL_PROFILE_PERFORMANCE:
		*profile = PLATFORM_PROFILE_PERFORMANCE;
		break;
	case HP_VICTUS_THERMAL_PROFILE_DEFAULT:
		*profile = PLATFORM_PROFILE_BALANCED;
		break;
	case HP_VICTUS_THERMAL_PROFILE_QUIET:
		*profile = PLATFORM_PROFILE_QUIET;
		break;
	default:
		return -EOPNOTSUPP;
	}

	return 0;
}

static int platform_profile_victus_get(PLATFORM_PROFILE_DEV_ARG,
				       enum platform_profile_option *profile)
{
	/* Same cached-value behaviour as platform_profile_omen_get() */
	return platform_profile_omen_get(PLATFORM_PROFILE_DEV_PASS, profile);
}

static int platform_profile_victus_set_ec(enum platform_profile_option profile)
{
	int err, tp;

	switch (profile) {
	case PLATFORM_PROFILE_PERFORMANCE:
		tp = HP_VICTUS_THERMAL_PROFILE_PERFORMANCE;
		break;
	case PLATFORM_PROFILE_BALANCED:
		tp = HP_VICTUS_THERMAL_PROFILE_DEFAULT;
		break;
	case PLATFORM_PROFILE_QUIET:
		tp = HP_VICTUS_THERMAL_PROFILE_QUIET;
		break;
	default:
		return -EOPNOTSUPP;
	}

	err = omen_thermal_profile_set(tp);
	return err < 0 ? err : 0;
}

static bool is_victus_s_thermal_profile(void)
{
	/* is_victus_s_board is initialised in driver init before this is called */
	return is_victus_s_board;
}

static int victus_s_gpu_thermal_profile_get(bool *ctgp_enable,
					    bool *ppab_enable,
					    u8 *dstate,
					    u8 *gpu_slowdown_temp)
{
	struct victus_gpu_power_modes gpu_power_modes = { 0 };
	int ret;

	ret = hp_wmi_perform_query(HPWMI_GET_GPU_THERMAL_MODES_QUERY, HPWMI_GM,
				   &gpu_power_modes, sizeof(gpu_power_modes),
				   sizeof(gpu_power_modes));
	if (ret == 0) {
		if (ctgp_enable)
			*ctgp_enable      = gpu_power_modes.ctgp_enable  ? true : false;
		if (ppab_enable)
			*ppab_enable      = gpu_power_modes.ppab_enable  ? true : false;
		if (dstate)
			*dstate           = gpu_power_modes.dstate;
		if (gpu_slowdown_temp)
			*gpu_slowdown_temp = gpu_power_modes.gpu_slowdown_temp;
	}

	return ret;
}

static int victus_s_gpu_thermal_profile_set(bool ctgp_enable,
					    bool ppab_enable,
					    u8 dstate)
{
	struct victus_gpu_power_modes gpu_power_modes;
	bool current_ctgp_state, current_ppab_state;
	u8 current_dstate, current_gpu_slowdown_temp;
	int ret;

	/* Read current slowdown temperature so we do not change it */
	ret = victus_s_gpu_thermal_profile_get(&current_ctgp_state,
					       &current_ppab_state,
					       &current_dstate,
					       &current_gpu_slowdown_temp);
	if (ret < 0) {
		pr_warn("GPU modes not updated, unable to get slowdown temp\n");
		return ret;
	}

	gpu_power_modes.ctgp_enable      = ctgp_enable ? 0x01 : 0x00;
	gpu_power_modes.ppab_enable      = ppab_enable ? 0x01 : 0x00;
	gpu_power_modes.dstate           = dstate;
	gpu_power_modes.gpu_slowdown_temp = current_gpu_slowdown_temp;

	return hp_wmi_perform_query(HPWMI_SET_GPU_THERMAL_MODES_QUERY, HPWMI_GM,
				    &gpu_power_modes, sizeof(gpu_power_modes), 0);
}

/* Pass HP_POWER_LIMIT_DEFAULT to restore firmware defaults for PL1/PL2 */
static int victus_s_set_cpu_pl1_pl2(u8 pl1, u8 pl2)
{
	struct victus_power_limits power_limits;

	if (pl1 == HP_POWER_LIMIT_NO_CHANGE || pl2 == HP_POWER_LIMIT_NO_CHANGE)
		return -EINVAL;

	/* PL2 must be >= PL1 */
	if (pl2 < pl1)
		return -EINVAL;

	power_limits.pl1                    = pl1;
	power_limits.pl2                    = pl2;
	power_limits.pl4                    = HP_POWER_LIMIT_NO_CHANGE;
	power_limits.cpu_gpu_concurrent_limit = HP_POWER_LIMIT_NO_CHANGE;

	return hp_wmi_perform_query(HPWMI_SET_POWER_LIMITS_QUERY, HPWMI_GM,
				    &power_limits, sizeof(power_limits), 0);
}

static int platform_profile_victus_s_get_ec(
	enum platform_profile_option *profile)
{
	bool current_ctgp_state, current_ppab_state;
	u8 current_dstate, current_gpu_slowdown_temp, tp;
	const struct thermal_profile_params *params;
	int ret;

	params = active_thermal_profile_params;
	if (!params)
		return -ENODEV;

	if (params->ec_tp_offset == HP_EC_OFFSET_UNKNOWN ||
	    params->ec_tp_offset == HP_NO_THERMAL_PROFILE_OFFSET) {
		*profile = active_platform_profile;
		return 0;
	}

	ret = ec_read(params->ec_tp_offset, &tp);
	if (ret)
		return ret;

	/*
	 * EC buggy cold-boot states: 0x00 (DEFAULT) and 0x01 (PERFORMANCE).
	 * Normal operational states: 0x30 (DEFAULT) and 0x31 (PERFORMANCE).
	 * Accept both sets so the driver works correctly right after a cold boot
	 * before the Omen Gaming Hub has had a chance to normalise the value.
	 */
	if (tp == HP_VICTUS_S_THERMAL_PROFILE_PERFORMANCE ||
	    tp == HP_OMEN_V1_THERMAL_PROFILE_PERFORMANCE) {
		*profile = PLATFORM_PROFILE_PERFORMANCE;
	} else if (tp == HP_VICTUS_S_THERMAL_PROFILE_DEFAULT ||
		   tp == HP_OMEN_V1_THERMAL_PROFILE_DEFAULT) {
		/*
		 * BALANCED and LOW_POWER both map to the same thermal profile
		 * value, so differentiate them by querying the GPU CTGP/PPAB
		 * power states.
		 */
		ret = victus_s_gpu_thermal_profile_get(&current_ctgp_state,
						       &current_ppab_state,
						       &current_dstate,
						       &current_gpu_slowdown_temp);
		if (ret < 0)
			return ret;

		if (!current_ctgp_state && !current_ppab_state)
			*profile = PLATFORM_PROFILE_LOW_POWER;
		else if (!current_ctgp_state && current_ppab_state)
			*profile = PLATFORM_PROFILE_BALANCED;
		else
			return -EINVAL;
	} else {
		return -EINVAL;
	}

	return 0;
}

static int platform_profile_victus_s_set_ec(
	enum platform_profile_option profile)
{
	const struct thermal_profile_params *params;
	bool gpu_ctgp_enable, gpu_ppab_enable;
	/* dstate: 1=100%, 2=50%, 3=25%, 4=12.5% */
	u8 gpu_dstate;
	int err, tp;

	params = active_thermal_profile_params;
	if (!params) {
		pr_warn("Thermal profile parameters not found, skipping operation\n");
		return -ENODEV;
	}

	switch (profile) {
	case PLATFORM_PROFILE_PERFORMANCE:
		tp               = params->performance;
		gpu_ctgp_enable  = true;
		gpu_ppab_enable  = true;
		gpu_dstate       = 1;
		break;
	case PLATFORM_PROFILE_BALANCED:
		tp               = params->balanced;
		gpu_ctgp_enable  = false;
		gpu_ppab_enable  = true;
		gpu_dstate       = 1;
		break;
	case PLATFORM_PROFILE_LOW_POWER:
		tp               = params->low_power;
		gpu_ctgp_enable  = false;
		gpu_ppab_enable  = false;
		gpu_dstate       = 1;
		break;
	default:
		return -EOPNOTSUPP;
	}

	hp_wmi_get_fan_count_userdefine_trigger();

	err = omen_thermal_profile_set(tp);
	if (err < 0) {
		pr_err("Failed to set platform profile %d: %d\n", profile, err);
		return err;
	}

	err = victus_s_gpu_thermal_profile_set(gpu_ctgp_enable, gpu_ppab_enable,
					       gpu_dstate);
	if (err < 0) {
		pr_err("Failed to set GPU profile %d: %d\n", profile, err);
		return err;
	}

	return 0;
}

static int platform_profile_victus_s_set(PLATFORM_PROFILE_DEV_ARG,
					 enum platform_profile_option profile)
{
	int err;

	guard(mutex)(&active_platform_profile_lock);

	err = platform_profile_victus_s_set_ec(profile);
	if (err < 0)
		return err;

	active_platform_profile = profile;
	return 0;
}

static int platform_profile_victus_set(PLATFORM_PROFILE_DEV_ARG,
				       enum platform_profile_option profile)
{
	int err;

	guard(mutex)(&active_platform_profile_lock);

	err = platform_profile_victus_set_ec(profile);
	if (err < 0)
		return err;

	active_platform_profile = profile;
	return 0;
}

static int hp_wmi_platform_profile_probe(void *drvdata,
					 unsigned long *choices)
{
	if (is_omen_thermal_profile()) {
		set_bit(PLATFORM_PROFILE_COOL, choices);
	} else if (is_victus_thermal_profile()) {
		set_bit(PLATFORM_PROFILE_QUIET, choices);
	} else if (is_victus_s_thermal_profile()) {
		/* ECO-mode equivalent from HP Omen software */
		set_bit(PLATFORM_PROFILE_LOW_POWER, choices);
	} else {
		set_bit(PLATFORM_PROFILE_QUIET, choices);
		set_bit(PLATFORM_PROFILE_COOL,  choices);
	}

	set_bit(PLATFORM_PROFILE_BALANCED,    choices);
	set_bit(PLATFORM_PROFILE_PERFORMANCE, choices);

	return 0;
}

static int omen_powersource_event(struct notifier_block *nb,
				  unsigned long value, void *data)
{
	struct acpi_bus_event *event_entry = data;
	enum platform_profile_option actual_profile;
	int err;

	if (strcmp(event_entry->device_class, ACPI_AC_CLASS) != 0)
		return NOTIFY_DONE;

	pr_debug("Received power source device event\n");

	guard(mutex)(&active_platform_profile_lock);

	/*
	 * This handler is only registered for Omen/Victus models, so
	 * is_omen_thermal_profile() / is_victus_thermal_profile() are stable.
	 */
	if (is_omen_thermal_profile())
		err = platform_profile_omen_get_ec(&actual_profile);
	else
		err = platform_profile_victus_get_ec(&actual_profile);

	if (err < 0) {
		pr_warn("Failed to read current platform profile (%d)\n", err);
		return NOTIFY_DONE;
	}

	/*
	 * If we are back on AC and the user-chosen profile differs from what
	 * the EC reports, restore the user's choice.
	 */
	if (power_supply_is_system_supplied() <= 0 ||
	    active_platform_profile == actual_profile) {
		pr_debug("Platform profile update skipped, conditions unmet\n");
		return NOTIFY_DONE;
	}

	if (is_omen_thermal_profile())
		err = platform_profile_omen_set_ec(active_platform_profile);
	else
		err = platform_profile_victus_set_ec(active_platform_profile);

	if (err < 0) {
		pr_warn("Failed to restore platform profile (%d)\n", err);
		return NOTIFY_DONE;
	}

	return NOTIFY_OK;
}

static int victus_s_powersource_event(struct notifier_block *nb,
				      unsigned long value, void *data)
{
	struct acpi_bus_event *event_entry = data;
	enum platform_profile_option actual_profile;
	int err;

	if (strcmp(event_entry->device_class, ACPI_AC_CLASS) != 0)
		return NOTIFY_DONE;

	pr_debug("Received power source device event\n");

	guard(mutex)(&active_platform_profile_lock);

	err = platform_profile_victus_s_get_ec(&actual_profile);
	if (err < 0) {
		pr_warn("Failed to read current platform profile (%d)\n", err);
		return NOTIFY_DONE;
	}

	/*
	 * Switching power sources while PERFORMANCE mode is active requires a
	 * manual CPU PL1/PL2 re-application.  Other modes self-correct.
	 * Observed on HP 16-s1034nf (board 8C9C) with F.11 and F.13 BIOS.
	 */
	if (actual_profile == PLATFORM_PROFILE_PERFORMANCE) {
		pr_debug("Triggering CPU PL1/PL2 actualization\n");
		err = victus_s_set_cpu_pl1_pl2(HP_POWER_LIMIT_DEFAULT,
					       HP_POWER_LIMIT_DEFAULT);
		if (err)
			pr_warn("Failed to actualize power limits: %d\n", err);

		return NOTIFY_DONE;
	}

	return NOTIFY_OK;
}

static int omen_register_powersource_event_handler(void)
{
	int err;

	platform_power_source_nb.notifier_call = omen_powersource_event;
	err = register_acpi_notifier(&platform_power_source_nb);
	if (err < 0) {
		pr_warn("Failed to install ACPI power source notify handler\n");
		return err;
	}

	return 0;
}

static int victus_s_register_powersource_event_handler(void)
{
	int err;

	platform_power_source_nb.notifier_call = victus_s_powersource_event;
	err = register_acpi_notifier(&platform_power_source_nb);
	if (err < 0) {
		pr_warn("Failed to install ACPI power source notify handler\n");
		return err;
	}

	return 0;
}

static inline void omen_unregister_powersource_event_handler(void)
{
	unregister_acpi_notifier(&platform_power_source_nb);
}

static inline void victus_s_unregister_powersource_event_handler(void)
{
	unregister_acpi_notifier(&platform_power_source_nb);
}

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
static const struct platform_profile_ops platform_profile_omen_ops = {
	.probe       = hp_wmi_platform_profile_probe,
	.profile_get = platform_profile_omen_get,
	.profile_set = platform_profile_omen_set,
};

static const struct platform_profile_ops platform_profile_victus_ops = {
	.probe       = hp_wmi_platform_profile_probe,
	.profile_get = platform_profile_victus_get,
	.profile_set = platform_profile_victus_set,
};

static const struct platform_profile_ops platform_profile_victus_s_ops = {
	.probe       = hp_wmi_platform_profile_probe,
	.profile_get = platform_profile_omen_get,   /* uses cached value */
	.profile_set = platform_profile_victus_s_set,
};

static const struct platform_profile_ops hp_wmi_platform_profile_ops = {
	.probe       = hp_wmi_platform_profile_probe,
	.profile_get = hp_wmi_platform_profile_get,
	.profile_set = hp_wmi_platform_profile_set,
};
#else
static struct platform_profile_handler platform_profile_omen_handler = {
	.profile_get = platform_profile_omen_get,
	.profile_set = platform_profile_omen_set,
};

static struct platform_profile_handler platform_profile_victus_handler = {
	.profile_get = platform_profile_victus_get,
	.profile_set = platform_profile_victus_set,
};

static struct platform_profile_handler platform_profile_victus_s_handler = {
	.profile_get = platform_profile_omen_get,
	.profile_set = platform_profile_victus_s_set,
};

static struct platform_profile_handler hp_wmi_platform_profile_handler = {
	.profile_get = hp_wmi_platform_profile_get,
	.profile_set = hp_wmi_platform_profile_set,
};
#endif

static int thermal_profile_setup(struct platform_device *device)
{
#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
	const struct platform_profile_ops *ops;
#else
	struct platform_profile_handler *handler;
#endif
	int err, tp;

	if (is_omen_thermal_profile()) {
		err = platform_profile_omen_get_ec(&active_platform_profile);
		if (err < 0) {
			pr_warn("Failed to read initial omen thermal profile (%d), defaulting to balanced\n", err);
			active_platform_profile = PLATFORM_PROFILE_BALANCED;
		}

		err = platform_profile_omen_set_ec(active_platform_profile);
		if (err < 0)
			pr_warn("Failed to apply initial omen thermal profile (%d), continuing\n", err);

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
		ops = &platform_profile_omen_ops;
#else
		handler = &platform_profile_omen_handler;
#endif

	} else if (is_victus_thermal_profile()) {
		err = platform_profile_victus_get_ec(&active_platform_profile);
		if (err < 0) {
			pr_warn("Failed to read initial thermal profile (%d), defaulting to balanced\n", err);
			active_platform_profile = PLATFORM_PROFILE_BALANCED;
		}

		err = platform_profile_victus_set_ec(active_platform_profile);
		if (err < 0)
			pr_warn("Failed to apply initial thermal profile (%d), continuing\n", err);

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
		ops = &platform_profile_victus_ops;
#else
		handler = &platform_profile_victus_handler;
#endif

	} else if (is_victus_s_thermal_profile()) {
		if (!active_thermal_profile_params) {
			pr_info("Thermal profile parameters not available, skipping platform profile setup\n");
			return 0;
		}

		/*
		 * For boards with an unknown EC layout, default to BALANCED to
		 * avoid reading uninitialised data via platform_profile_victus_s_get_ec().
		 */
		if (active_thermal_profile_params->ec_tp_offset == HP_EC_OFFSET_UNKNOWN ||
		    active_thermal_profile_params->ec_tp_offset ==
		    HP_NO_THERMAL_PROFILE_OFFSET) {
			active_platform_profile = PLATFORM_PROFILE_BALANCED;
		} else {
			err = platform_profile_victus_s_get_ec(&active_platform_profile);
			if (err < 0) {
				pr_warn("Failed to read initial thermal profile (%d), defaulting to balanced\n", err);
				active_platform_profile = PLATFORM_PROFILE_BALANCED;
			}
		}

		/* FIX: init-time error no longer blocks module loading */
		err = platform_profile_victus_s_set_ec(active_platform_profile);
		if (err < 0)
			pr_warn("Failed to apply initial thermal profile (%d), continuing\n", err);

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
		ops = &platform_profile_victus_s_ops;
#else
		handler = &platform_profile_victus_s_handler;
#endif

	} else {
		tp = thermal_profile_get();
		if (tp < 0)
			return tp;

		err = thermal_profile_set(tp);
		if (err)
			return err;

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
		ops = &hp_wmi_platform_profile_ops;
#else
		handler = &hp_wmi_platform_profile_handler;
#endif
	}

#if LINUX_VERSION_CODE >= KERNEL_VERSION(6, 14, 0)
	platform_profile_device =
		devm_platform_profile_register(&device->dev, "hp-wmi", NULL, ops);
	if (IS_ERR(platform_profile_device))
		return PTR_ERR(platform_profile_device);
#else
	err = platform_profile_register(handler);
	if (err)
		return err;
#endif

	pr_info("Registered as platform profile handler\n");
	platform_profile_support = true;

	return 0;
}

static int hp_wmi_hwmon_init(void);

static int __init hp_wmi_bios_setup(struct platform_device *device)
{
	int err;

	wifi_rfkill      = NULL;
	bluetooth_rfkill = NULL;
	wwan_rfkill      = NULL;
	rfkill2_count    = 0;

	/*
	 * Pre-2009 BIOS: command 0x1B returns 0x4, meaning BIOS has relinquished
	 * control of wireless power.
	 */
	if (!hp_wmi_bios_2009_later()) {
		if (hp_wmi_rfkill_setup(device))
			hp_wmi_rfkill2_setup(device);
	}

	err = hp_wmi_hwmon_init();
	if (err < 0)
		return err;

	err = thermal_profile_setup(device);
	if (err < 0)
		pr_warn("Failed to set up thermal profile (%d), fan control still available\n", err);

	return 0;
}

static void __exit hp_wmi_bios_remove(struct platform_device *device)
{
	struct hp_wmi_hwmon_priv *priv;
	int i;

	for (i = 0; i < rfkill2_count; i++) {
		rfkill_unregister(rfkill2[i].rfkill);
		rfkill_destroy(rfkill2[i].rfkill);
	}

	if (wifi_rfkill) {
		rfkill_unregister(wifi_rfkill);
		rfkill_destroy(wifi_rfkill);
	}
	if (bluetooth_rfkill) {
		rfkill_unregister(bluetooth_rfkill);
		rfkill_destroy(bluetooth_rfkill);
	}
	if (wwan_rfkill) {
		rfkill_unregister(wwan_rfkill);
		rfkill_destroy(wwan_rfkill);
	}

	priv = platform_get_drvdata(device);
	if (priv)
		cancel_delayed_work_sync(&priv->keep_alive_dwork);

	if (platform_profile_support) {
#if LINUX_VERSION_CODE < KERNEL_VERSION(6, 14, 0)
		platform_profile_remove();
#endif
	}
}

static int hp_wmi_resume_handler(struct device *device)
{
	if (hp_wmi_input_dev) {
		if (test_bit(SW_DOCK, hp_wmi_input_dev->swbit))
			input_report_switch(hp_wmi_input_dev, SW_DOCK,
					    hp_wmi_get_dock_state());
		if (test_bit(SW_TABLET_MODE, hp_wmi_input_dev->swbit))
			input_report_switch(hp_wmi_input_dev, SW_TABLET_MODE,
					    hp_wmi_get_tablet_mode());
		input_sync(hp_wmi_input_dev);
	}

	if (rfkill2_count)
		hp_wmi_rfkill2_refresh();

	if (wifi_rfkill)
		rfkill_set_states(wifi_rfkill,
				  hp_wmi_get_sw_state(HPWMI_WIFI),
				  hp_wmi_get_hw_state(HPWMI_WIFI));
	if (bluetooth_rfkill)
		rfkill_set_states(bluetooth_rfkill,
				  hp_wmi_get_sw_state(HPWMI_BLUETOOTH),
				  hp_wmi_get_hw_state(HPWMI_BLUETOOTH));
	if (wwan_rfkill)
		rfkill_set_states(wwan_rfkill,
				  hp_wmi_get_sw_state(HPWMI_WWAN),
				  hp_wmi_get_hw_state(HPWMI_WWAN));

	return 0;
}

static const struct dev_pm_ops hp_wmi_pm_ops = {
	.resume  = hp_wmi_resume_handler,
	.restore = hp_wmi_resume_handler,
};

/*
 * hp_wmi_bios_remove() lives in .exit.text.  For drivers registered via
 * module_platform_driver_probe() this is fine because they cannot be unbound
 * at runtime.  Mark the driver struct with __refdata to prevent modpost from
 * issuing a section-mismatch warning.
 */
static struct platform_driver hp_wmi_driver __refdata = {
	.driver = {
		.name       = "hp-wmi",
		.pm         = &hp_wmi_pm_ops,
		.dev_groups = hp_wmi_groups,
	},
	.remove = __exit_p(hp_wmi_bios_remove),
};

/*
 * hp_wmi_apply_fan_settings - push the current priv->mode to hardware.
 *
 * Must be called with priv->hwmon_lock held, EXCEPT when invoked from
 * hp_wmi_hwmon_init() before the workqueue is started.
 *
 * For the PWM_MODE_AUTO case this function calls cancel_delayed_work()
 * (the *non-_sync* variant). This is deliberate: _sync would deadlock
 * when called from within the keep_alive_handler workqueue item itself.
 * External callers that need a synchronous cancel must call
 * cancel_delayed_work_sync() *before* taking hwmon_lock, then call this
 * function with the lock held (see the PWM_MODE_AUTO branch in
 * hp_wmi_hwmon_write()).
 */
static int hp_wmi_apply_fan_settings(struct hp_wmi_hwmon_priv *priv)
{
	int ret = 0;

	/* Skip no-op transitions in AUTO→AUTO */
	if (priv->mode == PWM_MODE_AUTO && priv->prev_mode == PWM_MODE_AUTO)
		return 0;

	switch (priv->mode) {
	case PWM_MODE_MAX:
		/*
		 * Some firmware revisions require this trigger before max-fan mode
		 * actually takes effect. It is harmless on boards that do not
		 * implement the command, so treat it as best-effort.
		 */
		hp_wmi_get_fan_count_userdefine_trigger();
		ret = hp_wmi_fan_speed_max_set(1);
		if (ret < 0)
			return ret;
		schedule_delayed_work(&priv->keep_alive_dwork,
				      secs_to_jiffies(KEEP_ALIVE_DELAY_SECS));
		break;

	case PWM_MODE_MANUAL:
		if (!priv->fan_speed_available)
			return -EOPNOTSUPP;
		hp_wmi_get_fan_count_userdefine_trigger();
		schedule_delayed_work(&priv->keep_alive_dwork,
				      secs_to_jiffies(KEEP_ALIVE_DELAY_SECS));
		ret = hp_wmi_fan_speed_set(priv,
					   priv->target_rpms[0],
					   priv->target_rpms[1]);
		break;

	case PWM_MODE_AUTO:
		/*
		 * Use the non-_sync cancel here: if we are running inside the
		 * keep_alive_handler work item, _sync would deadlock.
		 * External callers guarantee synchronous cancellation before
		 * reaching this path (see hp_wmi_hwmon_write).
		 */
		cancel_delayed_work(&priv->keep_alive_dwork);
		ret = hp_wmi_fan_speed_max_reset(priv);
		break;

	default:
		return -EINVAL;
	}

	if (ret >= 0)
		priv->prev_mode = priv->mode;

	return ret;
}

static umode_t hp_wmi_hwmon_is_visible(const void *data,
				       enum hwmon_sensor_types type,
				       u32 attr, int channel)
{
	const struct hp_wmi_hwmon_priv *priv = data;

	switch (type) {
	case hwmon_pwm:
		/* Only channel 0 carries a meaningful PWM value */
		if (channel != 0)
			return 0;
		if (attr == hwmon_pwm_input && !priv->fan_speed_available)
			return 0;
		return 0644;

	case hwmon_fan:
		if (attr == hwmon_fan_input) {
			if (priv->fan_speed_available) {
				if (priv->uses_victus_s_fan_commands ? (hp_wmi_get_fan_speed_victus_s(channel) >= 0) : (hp_wmi_get_fan_speed(channel) >= 0))
					return 0444;
			} else {
				if (hp_wmi_get_fan_speed(channel) >= 0)
					return 0444;
			}
		} else if (attr == hwmon_fan_max) {
			return priv->fan_speed_available ? 0444 : 0;
		} else if (attr == hwmon_fan_target) {
			return priv->fan_speed_available ? 0644 : 0;
		}
		break;

	default:
		return 0;
	}

	return 0;
}

static int hp_wmi_hwmon_read(struct device *dev, enum hwmon_sensor_types type,
			     u32 attr, int channel, long *val)
{
	struct hp_wmi_hwmon_priv *priv = dev_get_drvdata(dev);
	int rpm, ret;

	mutex_lock(&priv->hwmon_lock);

	switch (type) {
	case hwmon_fan:
		if (channel < 0 || channel > 1) {
			ret = -EINVAL;
			break;
		}
		if (attr == hwmon_fan_input) {
			rpm = (priv->fan_speed_available && priv->uses_victus_s_fan_commands)
			      ? hp_wmi_get_fan_speed_victus_s(channel)
			      : hp_wmi_get_fan_speed(channel);
			if (rpm < 0) {
				ret = rpm;
				break;
			}
			*val = rpm;
			ret  = 0;
		} else if (attr == hwmon_fan_max) {
			*val = priv->max_rpms[channel];
			ret  = 0;
		} else if (attr == hwmon_fan_target) {
			*val = priv->target_rpms[channel];
			ret  = 0;
		} else {
			ret = -EOPNOTSUPP;
		}
		break;

	case hwmon_pwm:
		if (attr == hwmon_pwm_input) {
			if (!priv->fan_speed_available) {
				ret = -EOPNOTSUPP;
				break;
			}
			if (priv->uses_victus_s_fan_commands)
				rpm = hp_wmi_get_fan_speed_victus_s(channel);
			else
				rpm = hp_wmi_get_fan_speed(channel);
			if (rpm < 0) {
				ret = rpm;
				break;
			}
			*val = rpm_to_pwm(rpm / 100, priv);
			ret  = 0;
			break;
		}
		/* hwmon_pwm_enable — return current mode */
		switch (priv->mode) {
		case PWM_MODE_MAX:
		case PWM_MODE_MANUAL:
		case PWM_MODE_AUTO:
			*val = priv->mode;
			ret  = 0;
			break;
		default:
			ret = -ENODATA; /* shouldn't happen */
			break;
		}
		break;

	default:
		ret = -EINVAL;
		break;
	}

	mutex_unlock(&priv->hwmon_lock);
	return ret;
}

static int hp_wmi_hwmon_write(struct device *dev, enum hwmon_sensor_types type,
			      u32 attr, int channel, long val)
{
	struct hp_wmi_hwmon_priv *priv = dev_get_drvdata(dev);
	int rpm, ret = 0;

	mutex_lock(&priv->hwmon_lock);

	switch (type) {
	case hwmon_pwm:
		if (attr == hwmon_pwm_input) {
			if (channel < 0 || channel > 1) {
				ret = -EINVAL;
				break;
			}
			if (!priv->fan_speed_available) {
				ret = -EOPNOTSUPP;
				break;
			}
			if (priv->mode != PWM_MODE_MANUAL) {
				ret = -EINVAL;
				break;
			}
			if (val == 0) {
				rpm = 0;
			} else {
				rpm = pwm_to_rpm(val, priv);
				rpm = clamp_val(rpm, priv->min_rpm, priv->max_rpm);
			}
			priv->target_rpms[0] = (u16)rpm * 100;
			priv->target_rpms[1] = (u16)(rpm > 0 ? (rpm + priv->gpu_delta) * 100 : 0);
			priv->pwm = val;
			ret = hp_wmi_apply_fan_settings(priv);
			break;
		}

		/* hwmon_pwm_enable */
		switch (val) {
		case PWM_MODE_MAX:
			priv->mode = PWM_MODE_MAX;
			ret = hp_wmi_apply_fan_settings(priv);
			break;

		case PWM_MODE_MANUAL:
			if (!priv->fan_speed_available) {
				ret = -EOPNOTSUPP;
				break;
			}
			if (priv->mode == PWM_MODE_MANUAL)
				break; /* already in MANUAL, preserve target_rpms */

			/*
			 * Seed target RPMs from the current fan speed so the
			 * transition to manual mode is smooth.
			 */
			if (priv->uses_victus_s_fan_commands)
				rpm = hp_wmi_get_fan_speed_victus_s(0);
			else
				rpm = hp_wmi_get_fan_speed(0);
			if (rpm >= 0)
				priv->target_rpms[0] = rpm;

			if (priv->uses_victus_s_fan_commands)
				rpm = hp_wmi_get_fan_speed_victus_s(1);
			else
				rpm = hp_wmi_get_fan_speed(1);
			if (rpm >= 0)
				priv->target_rpms[1] = rpm;

			priv->mode = PWM_MODE_MANUAL;
			ret = hp_wmi_apply_fan_settings(priv);
			break;

		case PWM_MODE_AUTO:
			if (priv->mode == PWM_MODE_AUTO)
				break; /* already in AUTO, nothing to do */

			/*
			 * We must guarantee the keep_alive work item has
			 * finished before updating state.  _sync cannot be
			 * called while holding hwmon_lock because the handler
			 * also takes that lock.  Drop the lock, cancel
			 * synchronously, then re-acquire.
			 *
			 * The window between unlock and relock is safe: no
			 * other write can change mode to non-AUTO because
			 * we are the only path that sets AUTO, and any
			 * concurrent write that changes mode to MAX/MANUAL
			 * will reschedule the dwork itself.
			 */
			mutex_unlock(&priv->hwmon_lock);
			cancel_delayed_work_sync(&priv->keep_alive_dwork);
			mutex_lock(&priv->hwmon_lock);

			priv->mode = PWM_MODE_AUTO;
			ret = hp_wmi_apply_fan_settings(priv);
			break;

		default:
			ret = -EINVAL;
			break;
		}
		break;

	case hwmon_fan:
		if (attr != hwmon_fan_target) {
			ret = -EOPNOTSUPP;
			break;
		}
		if (!priv->fan_speed_available) {
			ret = -EOPNOTSUPP;
			break;
		}
		if (channel < 0 || channel > 1 || val < 0) {
			ret = -EINVAL;
			break;
		}
		priv->target_rpms[channel] = val;
		if (priv->target_rpms[channel ^ 1] == 0)
			priv->target_rpms[channel ^ 1] = val;
		priv->mode = PWM_MODE_MANUAL;
		ret = hp_wmi_apply_fan_settings(priv);
		break;

	default:
		ret = -EOPNOTSUPP;
		break;
	}

	mutex_unlock(&priv->hwmon_lock);
	return ret;
}

static const struct hwmon_channel_info *const info[] = {
	HWMON_CHANNEL_INFO(fan,
			   HWMON_F_INPUT | HWMON_F_TARGET | HWMON_F_MAX,
			   HWMON_F_INPUT | HWMON_F_TARGET | HWMON_F_MAX),
	HWMON_CHANNEL_INFO(pwm, HWMON_PWM_ENABLE | HWMON_PWM_INPUT),
	NULL
};

static const struct hwmon_ops ops = {
	.is_visible = hp_wmi_hwmon_is_visible,
	.read       = hp_wmi_hwmon_read,
	.write      = hp_wmi_hwmon_write,
};

static const struct hwmon_chip_info chip_info = {
	.ops  = &ops,
	.info = info,
};

static void hp_wmi_hwmon_keep_alive_handler(struct work_struct *work)
{
	struct delayed_work *dwork = to_delayed_work(work);
	struct hp_wmi_hwmon_priv *priv =
		container_of(dwork, struct hp_wmi_hwmon_priv, keep_alive_dwork);

	mutex_lock(&priv->hwmon_lock);

	/*
	 * If mode was changed to AUTO while we were waiting, just exit.
	 * hp_wmi_apply_fan_settings() in AUTO mode calls cancel_delayed_work()
	 * (non-_sync), which cannot cancel a handler that has already started
	 * running.  Checking mode here closes that race.
	 */
	if (priv->mode == PWM_MODE_AUTO) {
		mutex_unlock(&priv->hwmon_lock);
		return;
	}

	/*
	 * Re-apply current settings; apply_fan_settings() reschedules the
	 * work for the next keep-alive interval.
	 */
	hp_wmi_apply_fan_settings(priv);

	mutex_unlock(&priv->hwmon_lock);
}

static void hp_wmi_set_fallback_fan_limits(struct hp_wmi_hwmon_priv *priv)
{
	priv->min_rpm             = 0;
	/* firmware uses units of 100 RPM (50 == 5000 RPM) */
	priv->max_rpm             = VICTUS_S_FALLBACK_MAX_RPM_FW;
	priv->gpu_delta           = 0;
	priv->max_rpms[0]         = VICTUS_S_FALLBACK_MAX_RPM;
	priv->max_rpms[1]         = VICTUS_S_FALLBACK_MAX_RPM;
	priv->target_rpms[0]      = 0;
	priv->target_rpms[1]      = 0;
	priv->prev_mode           = -1;
	priv->fan_speed_available = true;
}

static int hp_wmi_setup_fan_settings(struct hp_wmi_hwmon_priv *priv)
{
	u8 fan_data[128] = {0};
	struct victus_s_fan_table *fan_table;
	u8 min_rpm, max_rpm, gpu_delta;
	int cpu_rpm, gpu_rpm, ret;

	priv->mode = PWM_MODE_AUTO;

	/*
	 * Fan table queries and Victus S fan speed commands only apply to
	 * Victus S-series boards.
	 */
	if (is_omen_thermal_profile() && !is_victus_s_thermal_profile()) {
		pr_info("Omen board detected, setting fallback fan speed limits for manual mode\n");
		hp_wmi_set_fallback_fan_limits(priv);
		priv->uses_victus_s_fan_commands = false;
		return 0;
	}

	if (!is_victus_s_thermal_profile()) {
		if (force_fan_control_support) {
			pr_info("Forcing fan control support on non-Victus-S board\n");
			hp_wmi_set_fallback_fan_limits(priv);
			priv->uses_victus_s_fan_commands = false;
		}
		return 0;
	}

	/* OMEN_878A_FAN_PROBE_GUARD — FIX for GH#195 boot-hang (board 878A and siblings):
	 * Boards listed in victus_s_thermal_profile_boards[] purely so they get
	 * HP_NO_THERMAL_PROFILE_OFFSET (i.e. skip EC thermal-profile reads) must
	 * NOT run the Victus-S fan WMI probing below. On WMBA-abort-prone boards
	 * (878A/8BCD/8C75/8BAC/...) those probes (0x2F/0x2D/0x11) flood
	 * _SB.WMID.WMBA with AE_AML_BUFFER_LIMIT during early boot and can lock
	 * the EC. fan_speed_available=false also hard-blocks the 0x2E write path,
	 * so a mis-configured daemon cannot flood the EC at runtime either.
	 */
	if (active_thermal_profile_params &&
	    active_thermal_profile_params->ec_tp_offset == HP_NO_THERMAL_PROFILE_OFFSET) {
		pr_info("%s: board %s uses no-EC thermal params; skipping Victus-S fan probes and blocking 0x2E writes\n",
			KBUILD_MODNAME, dmi_get_system_info(DMI_BOARD_NAME));
		hp_wmi_set_fallback_fan_limits(priv);
		priv->uses_victus_s_fan_commands = false;
		priv->fan_speed_available = false;
		return 0;
	}

	/* 1. Try Victus-S fan table query */
	ret = hp_wmi_perform_query(HPWMI_VICTUS_S_GET_FAN_TABLE_QUERY, HPWMI_GM,
				   &fan_data, 4, sizeof(fan_data));
	if (ret == 0) {
		fan_table = (struct victus_s_fan_table *)fan_data;
		if (fan_table->header.num_fans > 0 &&
		    sizeof(struct victus_s_fan_table_header) +
		    sizeof(struct victus_s_fan_table_entry) *
		    fan_table->header.num_fans <= sizeof(fan_data)) {
			
			min_rpm = fan_table->entries[0].cpu_rpm;
			max_rpm = fan_table->entries[fan_table->header.num_fans - 1].cpu_rpm;

			/* Workaround for buggy WMI firmware on boards like 8BBE which return max_rpm=18 (1800 RPM) */
			if (max_rpm >= 30) {
				gpu_delta = (fan_table->entries[0].gpu_rpm > fan_table->entries[0].cpu_rpm)
					    ? fan_table->entries[0].gpu_rpm - fan_table->entries[0].cpu_rpm
					    : 0;
				
				priv->min_rpm = min_rpm;
				priv->max_rpm = max_rpm;
				priv->gpu_delta = gpu_delta;
				priv->max_rpms[0] = max_rpm * 100;
				priv->max_rpms[1] = (max_rpm + gpu_delta) * 100;
				priv->target_rpms[0] = 0;
				priv->target_rpms[1] = 0;
				priv->prev_mode      = -1;
				priv->fan_speed_available = true;
				priv->uses_victus_s_fan_commands = true;
				return 0;
			}
			pr_warn("Malformed or bogus fan table (max_rpm=%d), falling back to probing\n", max_rpm);
		}
		pr_warn("Malformed fan table, falling back to probing\n");
	}

	/* 2. Fallback: Probe Victus-S fan speed commands directly */
	cpu_rpm = hp_wmi_get_fan_speed_victus_s(CPU_FAN);
	gpu_rpm = hp_wmi_get_fan_speed_victus_s(GPU_FAN);
	if (cpu_rpm >= 0 && gpu_rpm >= 0) {
		pr_info("Fan table query unsupported, using fallback fan speed probing with safe limits\n");
		hp_wmi_set_fallback_fan_limits(priv);
		priv->uses_victus_s_fan_commands = true;
		return 0;
	}

	/* 3. Fallback: Probe standard Omen fan commands (for legacy/cross-over boards like 8BBE) */
	cpu_rpm = hp_wmi_get_fan_speed(CPU_FAN);
	gpu_rpm = hp_wmi_get_fan_speed(GPU_FAN);
	if (cpu_rpm >= 0 && gpu_rpm >= 0) {
		pr_info("Victus-S fan queries unsupported on this board, using standard Omen fan commands\n");
		hp_wmi_set_fallback_fan_limits(priv);
		priv->uses_victus_s_fan_commands = false;
		return 0;
	}

	/* 4. Total failure */
	if (force_fan_control_support) {
		pr_warn("Forcing fan control support despite probe failures, falling back to 5000 RPM safe limits\n");
		hp_wmi_set_fallback_fan_limits(priv);
		priv->uses_victus_s_fan_commands = false; /* Safest default */
		return 0;
	}

	pr_info("Fan controls unsupported on this board, manual fan control unavailable\n");
	return 0;
}

static int hp_wmi_hwmon_init(void)
{
	struct device *dev = &hp_wmi_platform_dev->dev;
	struct hp_wmi_hwmon_priv *priv;
	struct device *hwmon;
	int ret;

	priv = devm_kzalloc(dev, sizeof(*priv), GFP_KERNEL);
	if (!priv)
		return -ENOMEM;

	mutex_init(&priv->hwmon_lock);

	ret = hp_wmi_setup_fan_settings(priv);
	if (ret)
		return ret;

	INIT_DELAYED_WORK(&priv->keep_alive_dwork,
			  hp_wmi_hwmon_keep_alive_handler);
	platform_set_drvdata(hp_wmi_platform_dev, priv);

	hwmon = devm_hwmon_device_register_with_info(dev, "hp", priv,
						     &chip_info, NULL);
	if (IS_ERR(hwmon)) {
		dev_err(dev, "Could not register hp hwmon device\n");
		return PTR_ERR(hwmon);
	}

	/*
	 * Apply initial fan settings.  The workqueue is not yet running so
	 * we can call apply_fan_settings() without holding hwmon_lock.
	 */

	/* 
	 * [Patch by xcellsior]
	 * On boards whose EC layout has not been fully characterised
	 * (ec_tp_offset == HP_EC_OFFSET_UNKNOWN), an unsolicited fan-control
	 * WMI write at probe makes some SBIOS implementations disable the
	 * NVIDIA Dynamic Boost DC controller, capping GPU TGP at the base
	 * value (e.g. 80 W on the HP Omen Max 16, board 8D41). Defer
	 * fan-mode reconciliation to the first userspace pwm_enable write
	 * on these boards.
	 */
	if (active_thermal_profile_params &&
	    active_thermal_profile_params->ec_tp_offset == HP_EC_OFFSET_UNKNOWN)
		return 0;

	hp_wmi_apply_fan_settings(priv);

	return 0;
}

static void __init setup_active_thermal_profile_params(void)
{
	const struct dmi_system_id *id;

	id = dmi_first_match(victus_s_thermal_profile_boards);
	if (!id)
		return;

	is_victus_s_board = true;
	active_thermal_profile_params = id->driver_data;

	if (active_thermal_profile_params->ec_tp_offset == HP_EC_OFFSET_UNKNOWN)
		pr_warn("Unknown EC layout for board %s. Thermal profile readback will be disabled. "
			"Please report this to platform-driver-x86@vger.kernel.org\n",
			dmi_get_system_info(DMI_BOARD_NAME));
}
static const struct dmi_system_id broken_omen_hpc_guid_boards[] __initconst = {
	{
		.matches = { DMI_MATCH(DMI_BOARD_NAME, "8BCD") },
	},
	{
		.matches = { DMI_MATCH(DMI_BOARD_NAME, "8C75") },
	},
	{
		.matches = { DMI_MATCH(DMI_BOARD_NAME, "8BAC") },
	},
	{
		.matches = { DMI_MATCH(DMI_BOARD_NAME, "878A") },
	},
	{
		.matches = { DMI_MATCH(DMI_BOARD_NAME, "8DD0") },
	},
	{}
};

static int __init hp_wmi_init(void)
{
	int event_capable = wmi_has_guid(HPWMI_EVENT_GUID);
	int bios_capable;
	int err, tmp = 0;

	if (wmi_has_guid(HPWMI_OMEN_HPC_GUID) && !dmi_check_system(broken_omen_hpc_guid_boards)) {
		active_bios_guid = HPWMI_OMEN_HPC_GUID;
		bios_capable = 1;
	} else if (wmi_has_guid(HPWMI_BIOS_GUID)) {
		active_bios_guid = HPWMI_BIOS_GUID;
		bios_capable = 1;
	} else {
		bios_capable = 0;
	}

	if (!bios_capable && !event_capable)
		return -ENODEV;

	if (hp_wmi_perform_query(HPWMI_HARDWARE_QUERY, HPWMI_READ, &tmp,
				 sizeof(tmp), sizeof(tmp)) ==
	    HPWMI_RET_INVALID_PARAMETERS)
		zero_insize_support = true;

	if (event_capable) {
		err = hp_wmi_input_setup();
		if (err) {
			pr_warn("hp_wmi_input_setup failed (%d), continuing without event handling\n", err);
			event_capable = 0;
		}
	}

	if (bios_capable) {
		u8 supported;
		has_mux = (hp_wmi_get_mux_supported_modes(&supported) == 0);

		hp_wmi_platform_dev = platform_device_register_simple(
			"hp-wmi", PLATFORM_DEVID_NONE, NULL, 0);
		if (IS_ERR(hp_wmi_platform_dev)) {
			err = PTR_ERR(hp_wmi_platform_dev);
			goto err_destroy_input;
		}

		setup_active_thermal_profile_params();

		err = platform_driver_probe(&hp_wmi_driver, hp_wmi_bios_setup);
		if (err)
			goto err_unregister_device;
	}

	if (is_omen_thermal_profile() || is_victus_thermal_profile()) {
		err = omen_register_powersource_event_handler();
		if (err)
			goto err_unregister_device;
	} else if (is_victus_s_thermal_profile()) {
		err = victus_s_register_powersource_event_handler();
		if (err)
			goto err_unregister_device;
	}

	return 0;

err_unregister_device:
	platform_device_unregister(hp_wmi_platform_dev);
err_destroy_input:
	if (event_capable)
		hp_wmi_input_destroy();
	return err;
}
module_init(hp_wmi_init);

static void __exit hp_wmi_exit(void)
{
	if (is_omen_thermal_profile() || is_victus_thermal_profile())
		omen_unregister_powersource_event_handler();
	else if (is_victus_s_thermal_profile())
		victus_s_unregister_powersource_event_handler();

	if (wmi_has_guid(HPWMI_EVENT_GUID))
		hp_wmi_input_destroy();

	if (camera_shutter_input_dev)
		input_unregister_device(camera_shutter_input_dev);

	if (hp_wmi_platform_dev) {
		platform_device_unregister(hp_wmi_platform_dev);
		platform_driver_unregister(&hp_wmi_driver);
	}
}
module_exit(hp_wmi_exit);
