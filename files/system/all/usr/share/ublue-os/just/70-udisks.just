# Workaround udisks2 pour montage USB (anti-race condition)
# Usage: just udisks-install | just udisks-uninstall

# Variables
sleep_time := "3"

# Install (sleep 3s + flock)
udisks-install:
	#!/usr/bin/env bash
	set -e

	pkexec bash -c '{
		echo '\''ACTION=="add", SUBSYSTEM=="block", ENV{DEVPATH}=="*usb*", ENV{DEVTYPE}=="partition", TAG+="systemd", ENV{SYSTEMD_WANTS}="udisks2-restart.service"'\'' > '\''/etc/udev/rules.d/99-restart-udisks.rules'\''
		echo '\''[Unit]'\'' > '\''/etc/systemd/system/udisks2-restart.service'\''
		echo '\''Description=Restart udisks2 apres USB (anti-race)'\'' >> '\''/etc/systemd/system/udisks2-restart.service'\''
		echo '\''DefaultDependencies=no'\'' >> '\''/etc/systemd/system/udisks2-restart.service'\''
		echo '\'''\'' >> '\''/etc/systemd/system/udisks2-restart.service'\''
		echo '\''[Service]'\'' >> '\''/etc/systemd/system/udisks2-restart.service'\''
		echo '\''Type=oneshot'\'' >> '\''/etc/systemd/system/udisks2-restart.service'\''
		echo '\''ExecStart=/bin/bash -c "flock -w 1 /var/lock/udisks-restart.lock sleep {{sleep_time}}; systemctl restart udisks2.service"'\'' >> '\''/etc/systemd/system/udisks2-restart.service'\''
		udevadm control --reload-rules
		udevadm trigger
		systemctl daemon-reload
	}'

	echo "Règle udev créée."
	echo "Service systemd créé avec délai de {{sleep_time}} secondes et verrou."
	echo "Installation terminée. Consultez les logs avec : journalctl -u udisks2 -f"

# Uninstall
udisks-uninstall:
	#!/usr/bin/env bash
	set -e

	pkexec bash -c '{
		rm -f '\''/etc/udev/rules.d/99-restart-udisks.rules'\''
		rm -f '\''/etc/systemd/system/udisks2-restart.service'\''
		rm -f '\''/var/lock/udisks-restart.lock'\''
		udevadm control --reload-rules
		systemctl daemon-reload
	}'

	echo "Règle udev supprimée."
	echo "Service supprimé."
	echo "Désinstallation terminée."
