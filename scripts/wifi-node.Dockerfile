# An OpenWrt rootfs with the Wi-Fi and batman-adv user space, for the hwsim lab
# (scripts/wifi-lab.sh). The kernel side (mac80211_hwsim, batman-adv) is the host's.
ARG IMAGE
FROM ${IMAGE}
ARG PACKAGES="wpad-mbedtls wifi-scripts iw iwinfo rpcd-mod-iwinfo ucode-mod-nl80211 ucode-mod-rtnl kmod-batman-adv batctl-default"
# Kernel modules can't load in a build container, so their install scripts fail; the check below
# makes sure the user space arrived anyway. /tmp/resolv.conf is a mount while building. procd
# jails dnsmasq with ujail, which can't make namespaces in a container; without it, procd runs
# services unjailed.
RUN mkdir -p /var/lock /var/run && \
    if [ -x /usr/bin/apk ]; then \
      apk update && apk add $PACKAGES || true; \
    else \
      opkg update && opkg install $PACKAGES || true; \
    fi && \
    for f in /usr/sbin/hostapd /usr/sbin/wpa_supplicant /usr/sbin/iw /usr/sbin/batctl \
      /usr/lib/ucode/nl80211.so /usr/lib/ucode/rtnl.so /lib/netifd/proto/batadv.sh; do \
      [ -e "$f" ] || { echo "missing $f" >&2; exit 1; }; \
    done && \
    { rm -rf /tmp/* 2>/dev/null || true; } && \
    rm /sbin/ujail
