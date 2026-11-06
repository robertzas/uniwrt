#!/bin/sh
# uniwrt's speed test (docs/features/wan.md, SQM; D-076), sent over SSH and run on the gateway
# as `sh -s -- <phase> <device> <count> <streams> <bypass> <url> <target>`, one phase a run so
# the controller reports each live. Stock tools only: uclient-fetch, ping, sysfs.
#
#   phase    idle: <count> pings, 5 a second; down | up: <streams> transfers for <count> s
#   device   the WAN's layer-3 device, whose byte counters measure the line
#   bypass   1: SQM stopped for the phase (the line's own rates), started again on any exit
#   url      what the transfers fetch, or post to; target: the host pinged
#
# Prints JSON lines: {"t":<centiseconds of uptime>,"rx":<bytes>,"tx":<bytes>} each second
# while loading, {"rtt":<ms>} per ping reply, {"err":"<code>"} when it can't run.

set -u
phase=$1 dev=$2 count=$3 streams=$4 bypass=$5 url=$6 target=$7
stats=/sys/class/net/$dev/statistics
work=/tmp/uniwrt-speedtest
loops=""

[ -r "$stats/rx_bytes" ] || { echo '{"err":"no_device"}'; exit 1; }
fetch=$(command -v uclient-fetch) || { echo '{"err":"no_fetch"}'; exit 1; }

cleanup() {
	for pid in $loops; do kill "$pid" 2>/dev/null; done
	for file in "$work"/pid.*; do
		[ -f "$file" ] && kill "$(cat "$file")" 2>/dev/null
	done
	rm -rf "$work"
	if [ "$bypass" = 1 ] && [ -x /etc/init.d/sqm ]; then
		/etc/init.d/sqm start >/dev/null 2>&1
	fi
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

rm -rf "$work"
mkdir -p "$work"
if [ "$bypass" = 1 ] && [ -x /etc/init.d/sqm ]; then
	/etc/init.d/sqm stop >/dev/null 2>&1
fi

rtts() {
	sed -n 's/.*time=\([0-9.]*\) ms.*/{"rtt":\1}/p'
}

# Uptime in centiseconds: /proc/uptime has two decimals.
now() {
	read -r up _ </proc/uptime
	echo "${up%.*}${up#*.}"
}

# One stream: transfer after transfer until killed, each transfer's pid in a file.
load() {
	while :; do
		if [ "$phase" = down ]; then
			"$fetch" -q -O /dev/null "$url" >/dev/null 2>&1 &
		else
			"$fetch" -q -O /dev/null --post-file="$work/body" "$url" >/dev/null 2>&1 &
		fi
		echo $! >"$work/pid.$1"
		wait $!
	done
}

case $phase in
idle)
	ping -c "$count" -i 0.2 -W 1 "$target" 2>/dev/null | rtts
	;;
down | up)
	if [ "$phase" = up ]; then
		dd if=/dev/zero of="$work/body" bs=1024 count=2048 2>/dev/null
	fi
	i=0
	while [ "$i" -lt "$streams" ]; do
		load "$i" &
		loops="$loops $!"
		i=$((i + 1))
	done
	ping -c "$count" -W 1 "$target" >"$work/ping" 2>/dev/null &
	loops="$loops $!"
	i=0
	while [ "$i" -le "$count" ]; do
		read -r rx <"$stats/rx_bytes"
		read -r tx <"$stats/tx_bytes"
		echo "{\"t\":$(now),\"rx\":$rx,\"tx\":$tx}"
		i=$((i + 1))
		[ "$i" -le "$count" ] && sleep 1
	done
	rtts <"$work/ping"
	;;
*)
	echo '{"err":"bad_phase"}'
	exit 1
	;;
esac
