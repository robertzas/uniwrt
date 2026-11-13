#!/bin/sh
# Lends the gateway's adoption port to a network for a moment (features/adoption.md, Path C;
# D-107): a device adopted on the port takes its lease on the site's LAN, the controller
# reaches it there, and the port goes back to the segment by itself. Nothing in uci changes:
# the segment goes down (`ifdown`, which frees the port: netifd's `remove_device` leaves a
# configured port where it is), netifd adds the port to the interface's bridge (`add_device`,
# its VLAN untagged where the bridge filters VLANs), and giving it back takes it out again and
# brings the segment up (`ifup`). A reboot restores the segment too. Checked on 24.10 and 25.12
# in the system harness.
#
#   lend <port> <interface> <seconds>   moves the port into <interface>'s bridge and watches it
#                                       in its own session; prints `lent`, or `busy`
#   keep <port>                         the lend lasts until the link goes down; prints `kept`
#   end <port>                          gives the port back now; prints `ended`
#
# The watcher gives the port back once <seconds> have passed (unless kept), or once the link
# has been down for DOWN seconds: a node's own reload may bounce it for a moment.

DOWN=10
POLL=2
SEGMENT=uw_adopt
STATE=/var/run/uniwrt-lend

name_ok() {
	case $1 in '' | *[!A-Za-z0-9._-]*) return 1 ;; esac
}

call() { # <interface> <add_device|remove_device> <port>
	ubus call "network.interface.$1" "$2" "{\"name\":\"$3\"}"
}

give_back() { # <port> <interface>
	call "$2" remove_device "$1"
	ifup "$SEGMENT"
	rm -f "$STATE/$1"
	logger -t uniwrt "the adoption port $1 is back on $SEGMENT"
}

watch() { # <port> <interface>
	down=0
	while [ -f "$STATE/$1" ]; do
		sleep "$POLL"
		read -r mode until _ <"$STATE/$1" 2>/dev/null || break
		if [ "$mode" = until ] && [ "$(date +%s)" -ge "$until" ]; then
			give_back "$1" "$2"
			return
		fi
		if [ "$(cat "/sys/class/net/$1/carrier" 2>/dev/null)" = 1 ]; then
			down=0
		else
			down=$((down + POLL))
			if [ "$down" -ge "$DOWN" ]; then
				give_back "$1" "$2"
				return
			fi
		fi
	done
}

command=$1
port=$2
name_ok "$port" || {
	echo "bad port" >&2
	exit 2
}
case $command in
lend)
	interface=$3
	seconds=$4
	name_ok "$interface" || {
		echo "bad interface" >&2
		exit 2
	}
	case $seconds in '' | *[!0-9]*) echo "bad seconds" >&2 && exit 2 ;; esac
	mkdir -p "$STATE"
	if [ -f "$STATE/$port" ]; then
		echo busy
		exit 0
	fi
	echo "until $(($(date +%s) + seconds)) $interface" >"$STATE/$port"
	ifdown "$SEGMENT"
	call "$interface" add_device "$port"
	# 24.10's netifd takes the port it adds for absent until the kernel reports it again, and
	# ifdown has just set it down.
	ip link set "$port" up
	# netifd answers before the port moves, and answers alike when it can't move it.
	tries=10
	until master=$(readlink "/sys/class/net/$port/master") && [ "${master##*/}" != br-adopt ]; do
		tries=$((tries - 1))
		if [ "$tries" -le 0 ]; then
			give_back "$port" "$interface"
			echo "netifd did not move $port" >&2
			exit 1
		fi
		sleep 1
	done
	logger -t uniwrt "the adoption port $port is lent to $interface for $seconds s"
	# The watcher runs this script again (the controller writes it to $STATE) in a session of its
	# own, which outlives the login that started it.
	setsid sh "$0" watch "$port" "$interface" </dev/null >/dev/null 2>&1 &
	echo lent
	;;
watch)
	trap '' HUP
	watch "$port" "$3"
	;;
keep)
	read -r _ _ interface <"$STATE/$port" 2>/dev/null || {
		echo "$port is not lent" >&2
		exit 1
	}
	echo "kept 0 $interface" >"$STATE/$port"
	echo kept
	;;
end)
	read -r _ _ interface <"$STATE/$port" 2>/dev/null || {
		echo ended
		exit 0
	}
	give_back "$port" "$interface"
	echo ended
	;;
*)
	echo "unknown command" >&2
	exit 2
	;;
esac
