//! "Bersih otomatis": a Windows Task Scheduler task that starts this
//! executable with `--auto-clean` at 20.00 on the chosen schedule.
//!
//! The task is registered from XML so the battery rule ("Hanya saat laptop
//! dicas") can be switched off; plain `schtasks /SC` always forbids battery.

use crate::settings::{Schedule, Settings};

const TASK_NAME: &str = r"MDN Cleaner\Bersih Otomatis";
pub const AUTO_CLEAN_ARG: &str = "--auto-clean";

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn trigger(schedule: Schedule) -> &'static str {
    match schedule {
        Schedule::Daily => "<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>",
        Schedule::Weekly => {
            "<ScheduleByWeek><DaysOfWeek><Sunday /></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>"
        }
        Schedule::Monthly => {
            "<ScheduleByMonth><DaysOfMonth><Day>1</Day></DaysOfMonth><Months><January /><February /><March /><April /><May /><June /><July /><August /><September /><October /><November /><December /></Months></ScheduleByMonth>"
        }
    }
}

fn task_xml(exe: &str, settings: &Settings) -> String {
    let (y, m, d) = crate::settings::local_date();
    let battery = if settings.only_on_ac { "true" } else { "false" };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>MDN Cleaner: analisis dan bersihkan sampah sesuai jadwal.</Description></RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{y:04}-{m:02}-{d:02}T20:00:00</StartBoundary>
      <Enabled>true</Enabled>
      {trigger}
    </CalendarTrigger>
  </Triggers>
  <Principals><Principal id="Author"><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>{battery}</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>{battery}</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT2H</ExecutionTimeLimit>
    <Enabled>true</Enabled>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>{AUTO_CLEAN_ARG}</Arguments></Exec></Actions>
</Task>
"#,
        trigger = trigger(settings.schedule),
        exe = xml_escape(exe),
    )
}

#[cfg(windows)]
pub fn set_enabled(enabled: bool, settings: &Settings) -> Result<(), String> {
    use crate::settings::run_windows_tool as run;
    if !enabled {
        // Turning off a task that was never created is not an error.
        let _ = run("schtasks", &["/Delete", "/TN", TASK_NAME, "/F"]);
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let xml = task_xml(&exe.to_string_lossy(), settings);

    // Task Scheduler reads UTF-16 XML; write it with a byte-order mark.
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(|u| u.to_le_bytes()));
    let path = std::env::temp_dir().join("mdn-cleaner-task.xml");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    let result = run("schtasks", &["/Create", "/TN", TASK_NAME, "/XML", &path.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&path);
    result
}

#[cfg(not(windows))]
pub fn set_enabled(_enabled: bool, _settings: &Settings) -> Result<(), String> {
    Err("Bersih otomatis hanya tersedia di Windows".into())
}
