BINDIR=/usr/local/bin

cli:
	cargo build --release

install: cli
	sudo cp -a target/release/fs-sentinel $(BINDIR)

sanoid-install: install zfs-linux/fs-sentinel-daemon.service zfs-linux/fs-sentinel-sanoid-postsnapshot.sh zfs-linux/fs-sentinel-sanoid-presnapshot.sh zfs-linux/fs-sentinel-zfs-daemon.sh
	sudo cp -a zfs-linux/*.sh $(BINDIR)
	sudo cp -a zfs-linux/*.service /etc/systemd/system
	sudo systemctl daemon-reload
	sudo systemctl enable fs-sentinel-daemon.service
