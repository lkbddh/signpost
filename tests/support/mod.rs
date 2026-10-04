#![allow(dead_code)]
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

/// A throwaway session bus (`dbus-daemon --nofork`) in a temp dir; killed on drop.
pub struct PrivateBus {
    pub dir: tempfile::TempDir,
    child: Child,
    pub address: String,
}

impl PrivateBus {
    pub fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let services = dir.path().join("services");
        std::fs::create_dir(&services).unwrap();
        let conf = dir.path().join("bus.conf");
        std::fs::write(
            &conf,
            format!(
                r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN" "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig><type>session</type><listen>unix:dir={}</listen><servicedir>{}</servicedir>
<policy context="default"><allow send_destination="*" eavesdrop="true"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>"#,
                dir.path().display(),
                services.display()
            ),
        )
        .unwrap();
        let mut child = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", conf.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon must be installed for integration tests");
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        Self {
            dir,
            child,
            address: line.trim().to_owned(),
        }
    }

    pub async fn connect(&self) -> zbus::Connection {
        zbus::connection::Builder::address(self.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap()
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
