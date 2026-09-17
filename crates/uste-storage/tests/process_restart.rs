use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::process::ExitStatusExt,
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const CHILD_MARKER: &str = "USTE_T12_PROCESS_CHILD";
const DIRECTORY_PATH: &str = "USTE_T12_PROCESS_DIRECTORY";

#[test]
fn real_process_kill_and_reopen_synced_file() {
    if std::env::var_os(CHILD_MARKER).is_some() {
        child_writer();
        return;
    }

    let scratch = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&scratch).unwrap();
    let discriminator = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = scratch.join(format!(
        "uste-t12-restart-{}-{discriminator}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let directory_guard = TestDirectory(directory.clone());
    let executable = std::env::current_exe().unwrap();
    let child = Command::new(executable)
        .arg("--exact")
        .arg("real_process_kill_and_reopen_synced_file")
        .arg("--nocapture")
        .env(CHILD_MARKER, "1")
        .env(DIRECTORY_PATH, &directory)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = ChildGuard::new(child);

    let mut readiness = child.stderr().take().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut byte = [0_u8; 1];
        let result = readiness.read_exact(&mut byte).map(|()| byte);
        let _ = sender.send(result);
    });
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("child readiness timed out")
            .expect("child readiness pipe closed"),
        [b'R']
    );
    let status = child.kill_and_wait();
    assert_eq!(status.signal(), Some(9));
    assert_eq!(
        fs::read(directory.join("payload")).unwrap(),
        b"synced before SIGKILL"
    );
    drop(directory_guard);
}

fn child_writer() {
    let directory = std::path::PathBuf::from(std::env::var_os(DIRECTORY_PATH).unwrap());
    let mut payload = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("payload"))
        .unwrap();
    payload.write_all(b"synced before SIGKILL").unwrap();
    payload.sync_data().unwrap();
    File::open(&directory).unwrap().sync_all().unwrap();

    std::io::stderr().write_all(b"R").unwrap();
    std::io::stderr().flush().unwrap();

    loop {
        thread::sleep(Duration::from_millis(50));
    }
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn stderr(&mut self) -> &mut Option<std::process::ChildStderr> {
        &mut self.0.as_mut().unwrap().stderr
    }

    fn kill_and_wait(&mut self) -> ExitStatus {
        let mut child = self.0.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct TestDirectory(std::path::PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("payload"));
        let _ = fs::remove_dir(&self.0);
    }
}
