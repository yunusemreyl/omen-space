
# Module names
# When RGB_ONLY=1, only build the RGB lighting companion driver
# (used on kernel >=7.0 where stock hp-wmi already handles fan control)
ifdef RGB_ONLY
obj-m += hp-omen-extra.o
else
obj-m += hp-wmi.o
obj-m += hp-omen-extra.o
endif

# Kernel build directory
KDIR ?= /lib/modules/$(shell uname -r)/build

PKGNAME := $(shell grep -oP 'PACKAGE_NAME="\K[^"]+' dkms.conf 2>/dev/null || echo "hp-omen-extra")
VERSION := $(shell grep -oP 'PACKAGE_VERSION="\K[^"]+' dkms.conf 2>/dev/null || echo "1.3.5")

# Current directory
PWD := $(shell pwd)

# Detect if the running kernel was compiled with Clang
LLVM_DETECTED := $(shell grep -iq clang /proc/version && echo 1 || echo 0)
LLVM ?= $(LLVM_DETECTED)

# Default target
all: modules

modules:
ifeq ($(LLVM),1)
	$(MAKE) LLVM=1 -C $(KDIR) M=$(PWD) modules
else
	$(MAKE) -C $(KDIR) M=$(PWD) modules
endif

modules_install:
	$(MAKE) -C $(KDIR) M=$(PWD) modules_install

# Clean target
clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean
	rm -rf *.pkg.tar.zst

install: all
	$(MAKE) -C $(KDIR) M=$(PWD) modules_install
	depmod -a

install-dkms:
	dkms add .
	dkms build -m $(PKGNAME) -v $(VERSION)
	dkms install -m $(PKGNAME) -v $(VERSION)

install-arch:
	makepkg -si

uninstall:
	rm -f /lib/modules/$(shell uname -r)/extra/hp-wmi.ko
	rm -f /lib/modules/$(shell uname -r)/extra/hp-omen-extra.ko
	depmod -a

uninstall-dkms:
	dkms remove -m $(PKGNAME) -v $(VERSION) --all
	rm -rf /usr/src/$(PKGNAME)-$(VERSION)

.PHONY: all clean install uninstall install-dkms uninstall-dkms install-arch