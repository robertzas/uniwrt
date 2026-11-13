#!/bin/sh
# uniwrt's first-boot script for zero-touch provisioning (features/adoption.md, Path B): runs
# once from /etc/uci-defaults on the first boot of an image. It holds no secrets (D-022): only
# the controller's public key and where to find it. It makes the device safe to plug in anywhere
# (a DHCP client on its LAN bridge, no DHCP server, Wi-Fi off), lets in only the controller's
# key, and installs a service that calls the controller home until it is adopted (D-102).
# The one-liner (Path D) runs it on a live device with a TOKEN: the hello carries it, which
# pre-approves the device, and the changes take effect once the script has ended.

# --- values (the controller writes these) ---
CONTROLLER_KEY='ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIExampleOnlyExampleOnlyExampleOnlyExam uniwrt'
URLS='https://uniwrt.lan:443'
ROLE='ap'
TOKEN=''
# --- end of values ---

# The controller's key, the only way in until adoption.
keys=/etc/dropbear/authorized_keys
if ! grep -qxF "$CONTROLLER_KEY" "$keys" 2>/dev/null; then
	(umask 077 && printf '%s\n' "$CONTROLLER_KEY" >>"$keys")
fi

# Root gets a random password that never leaves the device, so nobody logs in without the key
# (SSH or LuCI). Adoption sets one of its own, kept in the controller's vault.
password=$(hexdump -n 18 -e '18/1 "%02x"' /dev/urandom)
printf '%s\n%s\n' "$password" "$password" | passwd root >/dev/null 2>&1
password=''

# The LAN bridge asks for an address and serves none. The device asks the DNS server its lease
# names, not its own dnsmasq: that one keeps `.lan` local and drops private answers, so it
# would never find uniwrt.lan.
uci -q batch <<'EOF'
set network.lan.proto='dhcp'
delete network.lan.ipaddr
delete network.lan.netmask
delete network.lan.ip6assign
set dhcp.lan.ignore='1'
set dhcp.lan.dhcpv6='disabled'
set dhcp.lan.ra='disabled'
set dhcp.@dnsmasq[0].localuse='0'
EOF

# An access point or a bridge joins its WAN port to the bridge, as its first apply would: any
# port then reaches the controller. A gateway keeps its WAN.
if [ "$ROLE" != gateway ]; then
	wan=$(uci -q get network.wan.device)
	bridge=''
	for section in $(uci -q show network | sed -n 's/^network\.\([^.=]*\)=device$/\1/p'); do
		[ "$(uci -q get "network.$section.name")" = br-lan ] && bridge=$section
	done
	if [ -n "$wan" ] && [ -n "$bridge" ] && [ "${wan#@}" = "$wan" ]; then
		uci add_list "network.$bridge.ports=$wan"
		uci -q delete network.wan
		uci -q delete network.wan6
	fi
fi

# Wi-Fi stays off until the controller configures it.
for radio in $(uci -q show wireless | sed -n 's/^wireless\.\([^.=]*\)=wifi-device$/\1/p'); do
	uci set "wireless.$radio.disabled=1"
done

# The hostname from the label MAC (or the first port's): uniwrt-<its last 4 hex digits>.
mac=$(jsonfilter -q -i /etc/board.json -e '@.system.label_macaddr')
[ -n "$mac" ] || mac=$(cat /sys/class/net/eth0/address 2>/dev/null)
mac=$(printf '%s' "$mac" | tr -d ':' | tr 'A-F' 'a-f')
if [ ${#mac} -eq 12 ]; then
	uci set "system.@system[0].hostname=uniwrt-${mac#????????}"
fi
uci commit

# The call home.
mkdir -p /etc/uniwrt
printf "URLS='%s'\nROLE='%s'\nTOKEN='%s'\n" "$URLS" "$ROLE" "$TOKEN" >/etc/uniwrt/ztp.conf

# The controller writes node-agent/ztp/uniwrt-hello in here.
cat >/usr/sbin/uniwrt-hello <<'HELLO'
HELLO
chmod 755 /usr/sbin/uniwrt-hello

cat >/etc/init.d/uniwrt-hello <<'SERVICE'
#!/bin/sh /etc/rc.common
# Calls the uniwrt controller home until it adopts this device.
START=99
USE_PROCD=1

start_service() {
	procd_open_instance
	procd_set_param command /usr/sbin/uniwrt-hello
	procd_close_instance
}
SERVICE
chmod 755 /etc/init.d/uniwrt-hello
/etc/init.d/uniwrt-hello enable
if [ -z "$TOKEN" ]; then
	# The first boot: procd listed /etc/rc.d before this ran, so start it now.
	/etc/init.d/uniwrt-hello start
	exit 0
fi

# The one-liner on a live device: name the host key the controller will pin, then apply the
# changes and call home once the shell that ran this is done. The new address cuts its session,
# so the reload runs in its own session, deaf to the hangup.
fingerprint=$(dropbearkey -y -f /etc/dropbear/dropbear_ed25519_host_key 2>/dev/null |
	sed -n 's/^Fingerprint: //p')
echo "uniwrt: this device calls the controller home in a moment. Its host key is"
echo "uniwrt:   ${fingerprint:-unknown}"
echo "uniwrt: the controller shows the same one. This session ends once the network reloads."
setsid sh -c 'trap "" HUP; sleep 3; reload_config; sleep 10; /etc/init.d/uniwrt-hello start' \
	</dev/null >/dev/null 2>&1 &
exit 0
