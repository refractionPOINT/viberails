pub(crate) fn get_ppid() -> Option<u32> {
    use sysinfo::{ProcessRefreshKind, System, UpdateKind};

    let pid = sysinfo::get_current_pid().ok()?;
    let mut sys = System::new();
    sys.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        false,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Never),
    );
    sys.process(pid)?.parent().map(sysinfo::Pid::as_u32)
}
