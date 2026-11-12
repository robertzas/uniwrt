# An OpenWrt rootfs for the system harness (scripts/test-system.sh): the stock image plus
# `bridge`, to check the bridge's VLANs, a PPPoE server and client, for the ISP's end, and
# sqm-scripts, mwan3 and miniupnpd (off, as an install leaves them before a transaction turns
# them on), to check what render writes for SQM, failover (D-076) and UPnP (D-079), and
# batman-adv's user space, for the apply's wired backhaul (D-097; the kernel side is the host's).
ARG IMAGE
FROM ${IMAGE}
ARG PACKAGES="ip-bridge rp-pppoe-server ppp-mod-pppoe sqm-scripts kmod-sched-cake mwan3 miniupnpd-nftables"
# Kernel modules can't load in a build container, so batman-adv's install script fails: the
# check after it makes sure the user space arrived anyway. /tmp/resolv.conf is a mount while
# building. procd jails dnsmasq with ujail, which can't make namespaces in a container; without
# it, procd runs services unjailed.
RUN mkdir -p /var/lock /var/run && \
    if [ -x /usr/bin/apk ]; then \
      apk update && apk add $PACKAGES; \
    else \
      opkg update && opkg install $PACKAGES; \
    fi && \
    for f in /usr/sbin/bridge /usr/sbin/pppoe-server /usr/sbin/pppd /etc/init.d/sqm /usr/sbin/mwan3 /usr/sbin/miniupnpd; do \
      [ -e "$f" ] || { echo "missing $f" >&2; exit 1; }; \
    done && \
    /etc/init.d/sqm disable && /etc/init.d/mwan3 disable && /etc/init.d/miniupnpd disable && \
    BATMAN="kmod-batman-adv batctl-default" && \
    if [ -x /usr/bin/apk ]; then apk add $BATMAN || true; else opkg install $BATMAN || true; fi && \
    for f in /usr/sbin/batctl /lib/netifd/proto/batadv.sh; do \
      [ -e "$f" ] || { echo "missing $f" >&2; exit 1; }; \
    done && \
    { rm -rf /tmp/* 2>/dev/null || true; } && \
    rm /sbin/ujail
