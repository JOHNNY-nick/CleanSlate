//! Scheduled tasks (EF-07, D47): reading their definition, and the store
//! through which the quarantine exports, deletes and re-registers them.
//!
//! The real store is the COM Task Scheduler (`ITaskService`); engines built
//! for tests get [`NoTaskStore`], which refuses everything, so that no test
//! ever touches a real task.

use crate::error::{CoreError, CoreResult};

/// One action of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskAction {
    /// Starts a program (`<Exec><Command>`).
    Exec(String),
    /// COM handler, e-mail or message: no program path.
    Other(&'static str),
}

/// Text of a task definition file: UTF-16 LE with a byte order mark, else UTF-8.
pub fn decode_xml(bytes: &[u8]) -> String {
    match bytes {
        [0xFF, 0xFE, rest @ ..] => {
            // Pairs of bytes; an odd trailing byte is ignored.
            let (pairs, _) = rest.as_chunks::<2>();
            let units: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
            String::from_utf16_lossy(&units)
        }
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Unescapes the five predefined XML entities.
fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Actions of a task definition, in order.
pub fn parse_actions(xml: &str) -> Vec<TaskAction> {
    let Some(start) = xml.find("<Actions") else {
        return Vec::new();
    };
    let end = xml[start..]
        .find("</Actions>")
        .map_or(xml.len(), |e| start + e);
    let section = &xml[start..end];
    let mut actions = Vec::new();
    let mut rest = section;
    while let Some(i) = rest.find('<') {
        rest = &rest[i + 1..];
        let tag: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        match tag.as_str() {
            "Exec" => {
                let body_end = rest.find("</Exec>").unwrap_or(rest.len());
                let body = &rest[..body_end];
                let command = body
                    .split("<Command>")
                    .nth(1)
                    .and_then(|c| c.split("</Command>").next())
                    .map(unescape)
                    .unwrap_or_default();
                actions.push(TaskAction::Exec(command));
                rest = &rest[body_end..];
            }
            "ComHandler" => actions.push(TaskAction::Other("ComHandler")),
            "SendEmail" => actions.push(TaskAction::Other("SendEmail")),
            "ShowMessage" => actions.push(TaskAction::Other("ShowMessage")),
            _ => {}
        }
    }
    actions
}

/// Folder and name of a task path (`\Vendor\Updater` → `\Vendor`, `Updater`).
pub fn split_task_path(path: &str) -> Option<(String, String)> {
    let (folder, name) = path.rsplit_once('\\')?;
    if name.is_empty() || !path.starts_with('\\') {
        return None;
    }
    let folder = if folder.is_empty() { r"\" } else { folder };
    Some((folder.to_owned(), name.to_owned()))
}

/// Where tasks live: the quarantine reads, deletes and re-registers them.
pub trait TaskStore: Send + Sync {
    /// Full XML definition of the task at `path` (`\Vendor\Updater`).
    fn read(&self, path: &str) -> CoreResult<String>;
    /// Deletes the task.
    fn delete(&self, path: &str) -> CoreResult<()>;
    /// Registers `xml` at `path` (same folder, same name); fails if a task
    /// already exists there.
    fn register(&self, path: &str, xml: &str) -> CoreResult<()>;
    /// `true` if a task exists at `path`.
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_ok()
    }
}

/// Refuses everything (test engines).
#[derive(Debug, Default)]
pub struct NoTaskStore;

impl TaskStore for NoTaskStore {
    fn read(&self, path: &str) -> CoreResult<String> {
        Err(CoreError::InvalidInput(format!(
            "no task store on this engine: {path}"
        )))
    }
    fn delete(&self, path: &str) -> CoreResult<()> {
        self.read(path).map(|_| ())
    }
    fn register(&self, path: &str, _: &str) -> CoreResult<()> {
        self.read(path).map(|_| ())
    }
}

/// The Windows Task Scheduler, through COM (`ITaskService`).
#[derive(Debug, Default)]
pub struct ComTaskStore;

impl TaskStore for ComTaskStore {
    fn read(&self, path: &str) -> CoreResult<String> {
        com::read(path)
    }
    fn delete(&self, path: &str) -> CoreResult<()> {
        com::delete(path)
    }
    fn register(&self, path: &str, xml: &str) -> CoreResult<()> {
        com::register(path, xml)
    }
}

mod com {
    use windows::core::BSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_MULTITHREADED,
    };
    use windows::Win32::System::TaskScheduler::{
        ITaskFolder, ITaskService, TaskScheduler, TASK_CREATE, TASK_LOGON_NONE,
    };
    use windows::Win32::System::Variant::VARIANT;

    use super::split_task_path;
    use crate::error::{CoreError, CoreResult};

    /// Keeps COM initialized on this thread while the store is used.
    struct Apartment(bool);

    impl Apartment {
        fn enter() -> Self {
            // SAFETY: plain COM initialization; an apartment already set up
            // on this thread (RPC_E_CHANGED_MODE) is reused, not torn down.
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            Apartment(hr.is_ok())
        }
    }

    impl Drop for Apartment {
        fn drop(&mut self) {
            if self.0 {
                // SAFETY: balances the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
        }
    }

    fn failed(path: &str) -> impl Fn(windows::core::Error) -> CoreError + '_ {
        move |e| CoreError::InvalidInput(format!("task {path}: {e}"))
    }

    /// Folder of the task at `path`; created only when `create` (restore).
    fn folder(path: &str, create: bool) -> CoreResult<(Apartment, ITaskFolder, String)> {
        let apartment = Apartment::enter();
        let (folder, name) = split_task_path(path)
            .ok_or_else(|| CoreError::InvalidInput(format!("task path: {path}")))?;
        let empty = VARIANT::default();
        // SAFETY: COM calls on interfaces owned by this function; every
        // argument lives for the duration of the call.
        let folder = unsafe {
            let service: ITaskService =
                CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
                    .map_err(failed(path))?;
            service
                .Connect(&empty, &empty, &empty, &empty)
                .map_err(failed(path))?;
            match service.GetFolder(&BSTR::from(folder.as_str())) {
                Ok(f) => f,
                Err(e) if !create => return Err(failed(path)(e)),
                // Restoring into a folder deleted since: create it below the root.
                Err(_) => service
                    .GetFolder(&BSTR::from(r"\"))
                    .and_then(|root| root.CreateFolder(&BSTR::from(folder.as_str()), &empty))
                    .map_err(failed(path))?,
            }
        };
        Ok((apartment, folder, name))
    }

    pub(super) fn read(path: &str) -> CoreResult<String> {
        let (_apartment, folder, name) = folder(path, false)?;
        // SAFETY: see `folder`.
        unsafe {
            let task = folder
                .GetTask(&BSTR::from(name.as_str()))
                .map_err(failed(path))?;
            Ok(task.Xml().map_err(failed(path))?.to_string())
        }
    }

    pub(super) fn delete(path: &str) -> CoreResult<()> {
        let (_apartment, folder, name) = folder(path, false)?;
        // SAFETY: see `folder`.
        unsafe { folder.DeleteTask(&BSTR::from(name.as_str()), 0) }.map_err(failed(path))
    }

    pub(super) fn register(path: &str, xml: &str) -> CoreResult<()> {
        let (_apartment, folder, name) = folder(path, true)?;
        let empty = VARIANT::default();
        // SAFETY: see `folder`. TASK_CREATE fails if the task exists; the
        // principal (account, logon type) comes from the XML itself.
        unsafe {
            folder
                .RegisterTask(
                    &BSTR::from(name.as_str()),
                    &BSTR::from(xml),
                    TASK_CREATE.0,
                    &empty,
                    &empty,
                    TASK_LOGON_NONE,
                    &empty,
                )
                .map(|_| ())
                .map_err(failed(path))
        }
    }
}

/// In-memory store for the tests: no real task is ever touched.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FakeTaskStore(pub std::sync::Mutex<std::collections::BTreeMap<String, String>>);

#[cfg(test)]
impl TaskStore for FakeTaskStore {
    fn read(&self, path: &str) -> CoreResult<String> {
        self.0
            .lock()
            .unwrap()
            .get(&path.to_lowercase())
            .cloned()
            .ok_or_else(|| CoreError::NotFound {
                location: path.to_owned(),
            })
    }
    fn delete(&self, path: &str) -> CoreResult<()> {
        self.0
            .lock()
            .unwrap()
            .remove(&path.to_lowercase())
            .map(|_| ())
            .ok_or_else(|| CoreError::NotFound {
                location: path.to_owned(),
            })
    }
    fn register(&self, path: &str, xml: &str) -> CoreResult<()> {
        let mut tasks = self.0.lock().unwrap();
        if tasks.contains_key(&path.to_lowercase()) {
            return Err(CoreError::InvalidInput(format!("task exists: {path}")));
        }
        tasks.insert(path.to_lowercase(), xml.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_read_in_order() {
        let xml = r#"<Task><Actions Context="Author">
            <Exec><Command>"C:\Old &amp; Co\run.exe"</Command><Arguments>/q</Arguments></Exec>
            <ComHandler><ClassId>{1}</ClassId></ComHandler>
            <Exec><Command>C:\Two.exe</Command></Exec>
            <SendEmail /><ShowMessage /></Actions></Task>"#;
        assert_eq!(
            parse_actions(xml),
            [
                TaskAction::Exec(r#""C:\Old & Co\run.exe""#.into()),
                TaskAction::Other("ComHandler"),
                TaskAction::Exec(r"C:\Two.exe".into()),
                TaskAction::Other("SendEmail"),
                TaskAction::Other("ShowMessage"),
            ]
        );
        assert!(parse_actions("<Task />").is_empty());
    }

    #[test]
    fn task_paths_split_into_folder_and_name() {
        assert_eq!(
            split_task_path(r"\Vendor\Updater"),
            Some((r"\Vendor".into(), "Updater".into()))
        );
        assert_eq!(
            split_task_path(r"\Updater"),
            Some((r"\".into(), "Updater".into()))
        );
        assert_eq!(split_task_path(r"\Vendor\"), None);
        assert_eq!(split_task_path("Updater"), None);
    }

    #[test]
    fn utf16_and_utf8_definitions_decode() {
        let mut utf16 = vec![0xFF, 0xFE];
        utf16.extend("<Task/>".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode_xml(&utf16), "<Task/>");
        assert_eq!(decode_xml(b"\xEF\xBB\xBF<Task/>"), "<Task/>");
        assert_eq!(decode_xml(b"<Task/>"), "<Task/>");
    }

    #[test]
    fn test_store_refuses_and_fake_round_trips() {
        assert!(NoTaskStore.read(r"\X").is_err());
        assert!(NoTaskStore.delete(r"\X").is_err());
        assert!(NoTaskStore.register(r"\X", "<Task/>").is_err());
        let fake = FakeTaskStore::default();
        fake.register(r"\Vendor\X", "<Task/>").unwrap();
        assert!(fake.register(r"\Vendor\X", "<Task/>").is_err());
        assert!(fake.exists(r"\vendor\x"));
        fake.delete(r"\Vendor\X").unwrap();
        assert!(!fake.exists(r"\Vendor\X"));
    }
}
