use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

#[derive(Clone)]
pub struct TrayState {
    user_closed: Arc<AtomicBool>,
    auto_closed: Arc<AtomicBool>,
}

impl Default for TrayState {
    fn default() -> Self {
        Self {
            user_closed: Arc::new(AtomicBool::new(false)),
            auto_closed: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl TrayState {
    pub fn user_closed(&self) -> bool {
        self.user_closed.load(Ordering::Acquire)
    }

    pub fn mark_user_closed(&self) {
        self.user_closed.store(true, Ordering::Release);
        self.clear_auto_closed();
    }

    pub fn clear_user_closed(&self) {
        self.user_closed.store(false, Ordering::Release);
    }

    pub fn auto_closed(&self) -> bool {
        self.auto_closed.load(Ordering::Acquire)
    }

    pub fn mark_auto_closed(&self) {
        self.auto_closed.store(true, Ordering::Release);
    }

    pub fn clear_auto_closed(&self) {
        self.auto_closed.store(false, Ordering::Release);
    }
}

#[cfg(windows)]
mod windows {
    use super::TrayState;
    use crate::diagnostics::{Level, Logger};
    use std::{
        ffi::c_void,
        sync::{
            atomic::{AtomicU32, AtomicU8, Ordering},
            Arc,
        },
        thread,
    };
    use tauri::Manager;
    type Handle = *mut c_void;
    type Hwnd = *mut c_void;
    type WinEventHook = *mut c_void;

    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    #[repr(C)]
    struct TaskbarList {
        vtable: *const TaskbarListVTable,
    }

    #[repr(C)]
    struct TaskbarListVTable {
        query_interface: unsafe extern "system" fn(
            this: *mut TaskbarList,
            iid: *const Guid,
            object: *mut *mut c_void,
        ) -> i32,
        add_ref: unsafe extern "system" fn(this: *mut TaskbarList) -> u32,
        release: unsafe extern "system" fn(this: *mut TaskbarList) -> u32,
        hr_init: unsafe extern "system" fn(this: *mut TaskbarList) -> i32,
        add_tab: unsafe extern "system" fn(this: *mut TaskbarList, hwnd: Hwnd) -> i32,
        delete_tab: unsafe extern "system" fn(this: *mut TaskbarList, hwnd: Hwnd) -> i32,
        activate_tab: unsafe extern "system" fn(this: *mut TaskbarList, hwnd: Hwnd) -> i32,
        set_active_alt: unsafe extern "system" fn(this: *mut TaskbarList, hwnd: Hwnd) -> i32,
    }

    const CLSID_TASKBAR_LIST: Guid = Guid {
        data1: 0x56FDF344,
        data2: 0xFD6D,
        data3: 0x11D0,
        data4: [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90],
    };
    const IID_TASKBAR_LIST: Guid = Guid {
        data1: 0x56FDF342,
        data2: 0xFD6D,
        data3: 0x11D0,
        data4: [0x95, 0x8A, 0x00, 0x60, 0x97, 0xC9, 0xA0, 0x90],
    };
    const S_FALSE: i32 = 1;
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106u32 as i32;
    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const CLSCTX_INPROC_SERVER: u32 = 0x1;
    type WinEventProc = unsafe extern "system" fn(
        hook: WinEventHook,
        event: u32,
        hwnd: Hwnd,
        object: i32,
        child: i32,
        event_thread: u32,
        event_time: u32,
    );

    #[repr(C)]
    struct WinEventRecord {
        hook: WinEventHook,
        event: u32,
        hwnd: Hwnd,
        object: i32,
        child: i32,
        event_thread: u32,
        event_time: u32,
    }

    const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
    const EVENT_OBJECT_DESTROY: u32 = 0x8001;
    const EVENT_OBJECT_SHOW: u32 = 0x8002;
    const EVENT_OBJECT_HIDE: u32 = 0x8003;
    const OBJID_WINDOW: i32 = 0;
    const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
    const WINDOW_EVENT_MESSAGE: u32 = 0x8002;
    const PM_NOREMOVE: u32 = 0x0000;
    const GW_OWNER: u32 = 4;
    const GW_HWNDNEXT: u32 = 2;
    const GW_HWNDPREV: u32 = 3;
    const GA_ROOT: u32 = 2;
    const GWLP_HWNDPARENT: i32 = -8;
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_APPWINDOW: isize = 0x0004_0000;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 258;
    static EVENT_THREAD_ID: AtomicU32 = AtomicU32::new(0);

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct WinMessage {
        hwnd: Hwnd,
        message: u32,
        w_param: usize,
        l_param: isize,
        time: u32,
        point: Point,
    }

    impl Default for WinMessage {
        fn default() -> Self {
            Self {
                hwnd: std::ptr::null_mut(),
                message: 0,
                w_param: 0,
                l_param: 0,
                time: 0,
                point: Point { x: 0, y: 0 },
            }
        }
    }

    #[derive(Clone)]
    pub struct State(Arc<AtomicU8>);

    impl Default for State {
        fn default() -> Self {
            Self(Arc::new(AtomicU8::new(1)))
        }
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
        fn GetWindow(hwnd: Hwnd, command: u32) -> Hwnd;
        fn GetAncestor(hwnd: Hwnd, flags: u32) -> Hwnd;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
        fn GetForegroundWindow() -> Hwnd;
        fn IsIconic(hwnd: Hwnd) -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max_count: i32) -> i32;
        fn GetClassNameW(hwnd: Hwnd, text: *mut u16, max_count: i32) -> i32;
        fn SetWinEventHook(
            event_min: u32,
            event_max: u32,
            module: Hwnd,
            callback: Option<WinEventProc>,
            process_id: u32,
            thread_id: u32,
            flags: u32,
        ) -> WinEventHook;
        fn UnhookWinEvent(hook: WinEventHook) -> i32;
        fn PeekMessageW(
            message: *mut WinMessage,
            window: Hwnd,
            message_min: u32,
            message_max: u32,
            remove: u32,
        ) -> i32;
        fn GetMessageW(
            message: *mut WinMessage,
            window: Hwnd,
            message_min: u32,
            message_max: u32,
        ) -> i32;
        fn TranslateMessage(message: *const WinMessage) -> i32;
        fn DispatchMessageW(message: *const WinMessage) -> isize;
        fn PostThreadMessageW(thread_id: u32, message: u32, w_param: usize, l_param: isize) -> i32;
        fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
        fn SetWindowPos(
            hwnd: Hwnd,
            after: Hwnd,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
        fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetLastError() -> u32;
        fn SetLastError(error: u32);
        fn GetCurrentThreadId() -> u32;
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
        fn QueryFullProcessImageNameW(
            process: Handle,
            flags: u32,
            path: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoUninitialize();
        fn CoCreateInstance(
            class_id: *const Guid,
            outer: *mut c_void,
            context: u32,
            interface_id: *const Guid,
            object: *mut *mut c_void,
        ) -> i32;
    }

    fn window_thread_and_process_id(hwnd: Hwnd) -> (u32, u32) {
        let mut process_id = 0;
        let thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
        (thread_id, process_id)
    }

    fn window_process_id(hwnd: Hwnd) -> u32 {
        window_thread_and_process_id(hwnd).1
    }

    fn process_executable_name(process_id: u32) -> Option<String> {
        if process_id == 0 {
            return None;
        }
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if process.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let mut size = path.len() as u32;
        let ok =
            unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut size) } != 0;
        unsafe { CloseHandle(process) };
        ok.then(|| {
            String::from_utf16_lossy(&path[..size as usize])
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or_default()
                .to_string()
        })
    }

    fn process_is_alive(process_id: u32) -> bool {
        if process_id == 0 {
            return false;
        }
        let process = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                0,
                process_id,
            )
        };
        if process.is_null() {
            return false;
        }
        let result = unsafe { WaitForSingleObject(process, 100) };
        unsafe { CloseHandle(process) };
        result == WAIT_TIMEOUT
    }

    fn window_identity(hwnd: Hwnd) -> String {
        if hwnd.is_null() {
            return "hwnd=0".into();
        }
        let (thread_id, process_id) = window_thread_and_process_id(hwnd);
        let executable =
            process_executable_name(process_id).unwrap_or_else(|| "<unavailable>".into());
        format!(
            "hwnd=0x{:X},tid={},pid={},exe={}",
            hwnd as usize, thread_id, process_id, executable
        )
    }

    fn window_text(hwnd: Hwnd, class_name: bool) -> String {
        if hwnd.is_null() {
            return String::new();
        }
        let mut text = [0u16; 512];
        let length = unsafe {
            if class_name {
                GetClassNameW(hwnd, text.as_mut_ptr(), text.len() as i32)
            } else {
                GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32)
            }
        }
        .max(0) as usize;
        String::from_utf16_lossy(&text[..length.min(text.len())])
            .replace('\r', " ")
            .replace('\n', " ")
    }

    fn window_details(hwnd: Hwnd) -> String {
        if hwnd.is_null() {
            return "hwnd=0".into();
        }
        let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
        let previous = unsafe { GetWindow(hwnd, GW_HWNDPREV) };
        let next = unsafe { GetWindow(hwnd, GW_HWNDNEXT) };
        let mut rect = Rect::default();
        let rect_ok = unsafe { GetWindowRect(hwnd, &mut rect) } != 0;
        format!(
            "{} visible={} iconic={} owner={} previous={} next={} rect=({},{})-({},{}) rectOk={} exStyle=0x{:X} class={:?} title={:?}",
            window_identity(hwnd),
            unsafe { IsWindowVisible(hwnd) != 0 },
            unsafe { IsIconic(hwnd) != 0 },
            window_identity(owner),
            window_identity(previous),
            window_identity(next),
            rect.left,
            rect.top,
            rect.right,
            rect.bottom,
            rect_ok,
            unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) },
            window_text(hwnd, true),
            window_text(hwnd, false)
        )
    }

    fn is_codex_window(hwnd: Hwnd) -> bool {
        process_executable_name(window_process_id(hwnd))
            .is_some_and(|name| is_codex_executable_name(&name))
    }

    fn codex_root_window(hwnd: Hwnd) -> Hwnd {
        if hwnd.is_null() {
            return std::ptr::null_mut();
        }
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if !root.is_null() && is_codex_window(root) {
            root
        } else {
            std::ptr::null_mut()
        }
    }

    fn is_attached_codex_object(record: &WinEventRecord, attached_codex: Hwnd) -> bool {
        record.object == OBJID_WINDOW && !attached_codex.is_null() && record.hwnd == attached_codex
    }

    fn is_codex_executable_name(name: &str) -> bool {
        name.eq_ignore_ascii_case("codex.exe") || name.eq_ignore_ascii_case("chatgpt.exe")
    }

    fn set_level(pane: Hwnd, after: Hwnd, logger: &Logger) {
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_NOACTIVATE: u32 = 0x0010;
        let flags = SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE;
        let (result, last_error) = unsafe {
            SetLastError(0);
            let result = SetWindowPos(pane, after, 0, 0, 0, 0, flags);
            (result, GetLastError())
        };
        if logger.enabled(Level::Debug) {
            logger.debug(format!(
                "set_level pane=[{}] after={} result={} win32LastError={}",
                window_details(pane),
                after as isize,
                result,
                last_error
            ));
        }
    }

    fn set_taskbar_visibility(pane: Hwnd, visible: bool, logger: &Logger) {
        const SWP_NOSIZE: u32 = 0x0001;
        const SWP_NOMOVE: u32 = 0x0002;
        const SWP_NOZORDER: u32 = 0x0004;
        const SWP_NOACTIVATE: u32 = 0x0010;
        const SWP_FRAMECHANGED: u32 = 0x0020;
        let before = unsafe { GetWindowLongPtrW(pane, GWL_EXSTYLE) };
        let after = if visible {
            (before & !WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW
        } else {
            (before & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW
        };
        let (previous, position_result, last_error) = unsafe {
            SetLastError(0);
            let previous = SetWindowLongPtrW(pane, GWL_EXSTYLE, after);
            let position_result = SetWindowPos(
                pane,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
            (previous, position_result, GetLastError())
        };
        let taskbar_result = taskbar_tab(pane, visible);
        if let Err(error) = taskbar_result {
            logger.warn(format!(
                "set_taskbar_visibility taskbar operation failed visible={} hresult=0x{:X}",
                visible, error as u32
            ));
        }
        if logger.enabled(Level::Debug) {
            logger.debug(format!(
                "set_taskbar_visibility pane={} visible={} before=0x{:X} after=0x{:X} previous=0x{:X} positionResult={} taskbarResult={taskbar_result:?} win32LastError={}",
                window_identity(pane),
                visible,
                before,
                after,
                previous,
                position_result,
                last_error
            ));
        }
    }

    fn taskbar_tab(pane: Hwnd, visible: bool) -> Result<i32, i32> {
        const E_FAIL: i32 = 0x8000_4005u32 as i32;
        let initialize_result =
            unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED) };
        if initialize_result < 0 && initialize_result != RPC_E_CHANGED_MODE {
            return Err(initialize_result);
        }
        let should_uninitialize = initialize_result == 0 || initialize_result == S_FALSE;
        let mut raw = std::ptr::null_mut();
        let create_result = unsafe {
            CoCreateInstance(
                &CLSID_TASKBAR_LIST,
                std::ptr::null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_TASKBAR_LIST,
                &mut raw,
            )
        };
        if create_result < 0 || raw.is_null() {
            if should_uninitialize {
                unsafe { CoUninitialize() };
            }
            return Err(if create_result < 0 {
                create_result
            } else {
                E_FAIL
            });
        }
        let list = raw as *mut TaskbarList;
        let vtable = unsafe { (*list).vtable };
        let operation_result = if vtable.is_null() {
            E_FAIL
        } else {
            unsafe {
                let vtable = &*vtable;
                let hr_init = (vtable.hr_init)(list);
                if hr_init < 0 {
                    hr_init
                } else if visible {
                    (vtable.add_tab)(list, pane)
                } else {
                    (vtable.delete_tab)(list, pane)
                }
            }
        };
        if !vtable.is_null() {
            unsafe {
                ((*vtable).release)(list);
            }
        }
        if should_uninitialize {
            unsafe { CoUninitialize() };
        }
        if operation_result < 0 {
            Err(operation_result)
        } else {
            Ok(operation_result)
        }
    }

    fn set_owner(pane: Hwnd, owner: Hwnd, logger: &Logger) {
        let detailed = logger.enabled(Level::Debug);
        let before = detailed.then(|| window_details(pane));
        let (previous, last_error) = unsafe {
            SetLastError(0);
            let previous = SetWindowLongPtrW(pane, GWLP_HWNDPARENT, owner as isize);
            (previous, GetLastError())
        };
        if detailed {
            let current = unsafe { GetWindow(pane, GW_OWNER) };
            logger.debug(format!(
                "set_owner before=[{}] requested={} previous=0x{:X} current={} win32LastError={} after=[{}]",
                before.unwrap_or_default(),
                window_identity(owner),
                previous,
                window_identity(current),
                last_error,
                window_details(pane)
            ));
        }
    }

    fn pane_state(pane: Hwnd) -> String {
        window_details(pane)
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum PaneVisibilityAction {
        Show,
        Hide,
        Keep,
    }

    fn pane_visibility_action(
        visible: bool,
        iconic: bool,
        user_closed: bool,
        auto_closed: bool,
        codex_foreground: bool,
        codex_closed: bool,
    ) -> PaneVisibilityAction {
        if user_closed || codex_closed {
            return if visible {
                PaneVisibilityAction::Hide
            } else {
                PaneVisibilityAction::Keep
            };
        }
        if codex_foreground {
            if auto_closed && (!visible || iconic) {
                PaneVisibilityAction::Show
            } else {
                PaneVisibilityAction::Keep
            }
        } else {
            PaneVisibilityAction::Keep
        }
    }

    fn sync(
        pane: Hwnd,
        foreground: Hwnd,
        tray_state: &TrayState,
        logger: &Logger,
        codex_closed: bool,
    ) {
        const SW_HIDE: i32 = 0;
        const SW_SHOWNOACTIVATE: i32 = 4;
        let visible = unsafe { IsWindowVisible(pane) } != 0;
        let iconic = unsafe { IsIconic(pane) } != 0;
        let codex_root = codex_root_window(foreground);
        let codex_foreground = !codex_root.is_null();
        let user_closed = tray_state.user_closed();
        let auto_closed = tray_state.auto_closed();
        if codex_foreground && !user_closed && unsafe { GetWindow(pane, GW_OWNER) } != codex_root {
            set_owner(pane, codex_root, logger);
        }
        let action = pane_visibility_action(
            visible,
            iconic,
            user_closed,
            auto_closed,
            codex_foreground,
            codex_closed,
        );
        if logger.enabled(Level::Debug) {
            logger.debug(format!(
                "sync begin paneState=[{}] foreground=[{}] codexForeground={} userClosed={} autoClosed={} codexClosed={} action={:?}",
                pane_state(pane),
                window_details(foreground),
                codex_foreground,
                user_closed,
                auto_closed,
                codex_closed,
                action
            ));
        }
        match action {
            PaneVisibilityAction::Show => {
                let result = unsafe { ShowWindow(pane, SW_SHOWNOACTIVATE) };
                tray_state.clear_auto_closed();
                logger.info(format!(
                    "sync restored pane reason={} result={result}",
                    if codex_foreground {
                        "codex-foreground"
                    } else {
                        "foreground-state"
                    }
                ));
            }
            PaneVisibilityAction::Hide => {
                let result = unsafe { ShowWindow(pane, SW_HIDE) };
                logger.info(format!(
                    "sync closed pane window reason={}",
                    if user_closed {
                        "user-closed"
                    } else {
                        "codex-closed"
                    }
                ));
                if logger.enabled(Level::Debug) {
                    logger.debug(format!("sync close-to-tray result={result}"));
                }
            }
            PaneVisibilityAction::Keep => {}
        }
    }

    fn foreground_changed(previous: Hwnd, current: Hwnd) -> bool {
        previous != current
    }

    unsafe extern "system" fn on_window_event(
        hook: WinEventHook,
        event: u32,
        hwnd: Hwnd,
        object: i32,
        child: i32,
        event_thread: u32,
        event_time: u32,
    ) {
        let thread_id = EVENT_THREAD_ID.load(Ordering::Acquire);
        if thread_id != 0 {
            let record = Box::into_raw(Box::new(WinEventRecord {
                hook,
                event,
                hwnd,
                object,
                child,
                event_thread,
                event_time,
            }));
            let posted =
                unsafe { PostThreadMessageW(thread_id, WINDOW_EVENT_MESSAGE, record as usize, 0) };
            if posted == 0 {
                unsafe { drop(Box::from_raw(record)) };
            }
        }
    }

    pub fn start(app: tauri::AppHandle, state: State, tray_state: TrayState, logger: Logger) {
        thread::spawn(move || {
            logger.info(format!(
                "window attach thread start mode={}",
                state.0.load(Ordering::Relaxed)
            ));
            let mut message = WinMessage::default();
            unsafe {
                PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            }

            let thread_id = unsafe { GetCurrentThreadId() };
            EVENT_THREAD_ID.store(thread_id, Ordering::Release);
            let (foreground_hook, foreground_error) = unsafe {
                SetLastError(0);
                let hook = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    std::ptr::null_mut(),
                    Some(on_window_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                (hook, GetLastError())
            };
            let (object_hook, object_error) = unsafe {
                SetLastError(0);
                let hook = SetWinEventHook(
                    EVENT_OBJECT_DESTROY,
                    EVENT_OBJECT_HIDE,
                    std::ptr::null_mut(),
                    Some(on_window_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
                );
                (hook, GetLastError())
            };
            if foreground_hook.is_null() {
                if !object_hook.is_null() {
                    unsafe { UnhookWinEvent(object_hook) };
                }
                logger.error(format!(
                    "foreground event hook registration failed win32LastError={foreground_error}"
                ));
                EVENT_THREAD_ID.store(0, Ordering::Release);
                return;
            }
            logger.info(format!(
                "foreground event hook registered threadId={thread_id}"
            ));
            if object_hook.is_null() {
                logger.warn(format!(
                    "window object event hook registration failed win32LastError={object_error}"
                ));
            } else {
                logger.info("window object event hook registered");
            }

            let mut last_foreground = unsafe { GetForegroundWindow() };
            let mut attached_codex = codex_root_window(last_foreground);
            let mut attached_codex_process_id = if attached_codex.is_null() {
                0
            } else {
                window_process_id(attached_codex)
            };
            if logger.enabled(Level::Debug) {
                logger.debug(format!(
                    "initial foreground=[{}]",
                    window_details(last_foreground)
                ));
            }
            if state.0.load(Ordering::Relaxed) == 1 {
                match app.get_webview_window("main") {
                    Some(window) => match window.hwnd() {
                        Ok(raw) => {
                            set_taskbar_visibility(raw.0 as Hwnd, false, &logger);
                            sync(raw.0 as Hwnd, last_foreground, &tray_state, &logger, false)
                        }
                        Err(error) => {
                            logger.error(format!("initial pane hwnd lookup failed error={error}"))
                        }
                    },
                    None => logger.error("initial pane webview not found"),
                }
            }

            loop {
                let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
                if result == 0 {
                    logger.info("foreground event message loop received quit");
                    break;
                }
                if result < 0 {
                    logger.error(format!(
                        "foreground event message loop ended result={} win32LastError={}",
                        result,
                        unsafe { GetLastError() }
                    ));
                    break;
                }
                if message.message == WINDOW_EVENT_MESSAGE {
                    let event_record = if message.w_param == 0 {
                        None
                    } else {
                        Some(unsafe { Box::from_raw(message.w_param as *mut WinEventRecord) })
                    };
                    let event = event_record
                        .as_deref()
                        .map(|record| record.event)
                        .unwrap_or(EVENT_SYSTEM_FOREGROUND);
                    let attached_codex_object = event_record
                        .as_deref()
                        .is_some_and(|record| is_attached_codex_object(record, attached_codex));
                    if let Some(record) = event_record.as_deref() {
                        if logger.enabled(Level::Debug)
                            && (event == EVENT_SYSTEM_FOREGROUND || attached_codex_object)
                        {
                            logger.debug(format!(
                                "window event callback hook=0x{:X} event={} hwnd=[{}] object={} child={} eventThread={} eventTime={}",
                                record.hook as usize,
                                record.event,
                                window_details(record.hwnd),
                                record.object,
                                record.child,
                                record.event_thread,
                                record.event_time
                            ));
                        }
                    } else {
                        logger.warn("window event message missing callback record");
                    }
                    if event == EVENT_OBJECT_DESTROY {
                        let destroyed_codex = attached_codex_object;
                        if destroyed_codex {
                            let process_id = attached_codex_process_id;
                            let process_alive = process_is_alive(process_id);
                            logger.info(format!(
                                "attached Codex window destroyed hwnd={} processId={} processAlive={process_alive}",
                                window_identity(attached_codex),
                                process_id
                            ));
                            attached_codex = std::ptr::null_mut();
                            attached_codex_process_id =
                                process_alive.then_some(process_id).unwrap_or(0);
                            if process_alive
                                && state.0.load(Ordering::Relaxed) == 1
                                && !tray_state.user_closed()
                            {
                                tray_state.mark_auto_closed();
                                match app.get_webview_window("main") {
                                    Some(window) => match window.hwnd() {
                                        Ok(raw) => sync(
                                            raw.0 as Hwnd,
                                            unsafe { GetForegroundWindow() },
                                            &tray_state,
                                            &logger,
                                            true,
                                        ),
                                        Err(error) => logger.error(format!(
                                            "Codex close pane hwnd lookup failed error={error}"
                                        )),
                                    },
                                    None => logger.error("Codex close pane webview not found"),
                                }
                            } else if !process_alive {
                                logger.info("Codex process exited; pane close ignored");
                            }
                        }
                        continue;
                    }
                    if event == EVENT_OBJECT_SHOW || event == EVENT_OBJECT_HIDE {
                        if attached_codex_object {
                            let record = event_record.expect("Codex object event record");
                            let process_id = attached_codex_process_id;
                            if event == EVENT_OBJECT_HIDE {
                                let window_hidden = unsafe {
                                    IsWindowVisible(record.hwnd) == 0 && IsIconic(record.hwnd) == 0
                                };
                                let process_alive = window_hidden && process_is_alive(process_id);
                                logger.info(format!(
                                    "attached Codex root hide event hwnd={} processId={} processAlive={} windowHidden={window_hidden}",
                                    window_identity(record.hwnd),
                                    process_id,
                                    process_alive,
                                ));
                                if window_hidden
                                    && process_alive
                                    && state.0.load(Ordering::Relaxed) == 1
                                    && !tray_state.user_closed()
                                {
                                    tray_state.mark_auto_closed();
                                    match app.get_webview_window("main") {
                                        Some(window) => match window.hwnd() {
                                            Ok(raw) => sync(
                                                raw.0 as Hwnd,
                                                unsafe { GetForegroundWindow() },
                                                &tray_state,
                                                &logger,
                                                true,
                                            ),
                                            Err(error) => logger.error(format!(
                                                "Codex hide pane hwnd lookup failed error={error}"
                                            )),
                                        },
                                        None => logger.error("Codex hide pane webview not found"),
                                    }
                                } else if !process_alive {
                                    logger.info("Codex process exited; pane close ignored");
                                }
                            } else {
                                logger.info(format!(
                                    "attached Codex root show event hwnd={} processId={} autoClosed={} userClosed={}",
                                    window_identity(record.hwnd),
                                    process_id,
                                    tray_state.auto_closed(),
                                    tray_state.user_closed(),
                                ));
                                if tray_state.auto_closed()
                                    && !tray_state.user_closed()
                                    && state.0.load(Ordering::Relaxed) == 1
                                {
                                    match app.get_webview_window("main") {
                                        Some(window) => match window.hwnd() {
                                            Ok(raw) => sync(
                                                raw.0 as Hwnd,
                                                record.hwnd,
                                                &tray_state,
                                                &logger,
                                                false,
                                            ),
                                            Err(error) => logger.error(format!(
                                                "Codex show pane hwnd lookup failed error={error}"
                                            )),
                                        },
                                        None => logger.error("Codex show pane webview not found"),
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    let foreground = event_record
                        .as_deref()
                        .map(|record| record.hwnd)
                        .filter(|hwnd| !hwnd.is_null())
                        .unwrap_or_else(|| unsafe { GetForegroundWindow() });
                    if foreground_changed(last_foreground, foreground) {
                        logger.info(format!(
                            "foreground changed previous={} current={}",
                            window_identity(last_foreground),
                            window_identity(foreground)
                        ));
                        if logger.enabled(Level::Debug) {
                            logger.debug(format!(
                                "foreground changed details previous=[{}] current=[{}]",
                                window_details(last_foreground),
                                window_details(foreground)
                            ));
                        }
                        last_foreground = foreground;
                        let codex_root = codex_root_window(foreground);
                        if !codex_root.is_null() {
                            attached_codex = codex_root;
                            attached_codex_process_id = window_process_id(codex_root);
                        }
                        let codex_window_closed = if codex_root.is_null()
                            && !attached_codex.is_null()
                            && !tray_state.auto_closed()
                            && unsafe {
                                IsWindowVisible(attached_codex) == 0
                                    && IsIconic(attached_codex) == 0
                            } {
                            let process_id = attached_codex_process_id;
                            let process_alive = process_is_alive(process_id);
                            logger.info(format!(
                                "attached Codex window closed hwnd={} processId={} processAlive={process_alive}",
                                window_identity(attached_codex),
                                process_id
                            ));
                            attached_codex = std::ptr::null_mut();
                            attached_codex_process_id =
                                process_alive.then_some(process_id).unwrap_or(0);
                            process_alive
                        } else {
                            false
                        };
                        if state.0.load(Ordering::Relaxed) == 1 {
                            match app.get_webview_window("main") {
                                Some(window) => match window.hwnd() {
                                    Ok(raw) => {
                                        if codex_window_closed && !tray_state.user_closed() {
                                            tray_state.mark_auto_closed();
                                        }
                                        sync(
                                            raw.0 as Hwnd,
                                            foreground,
                                            &tray_state,
                                            &logger,
                                            codex_window_closed,
                                        )
                                    }
                                    Err(error) => logger.error(format!(
                                        "foreground change pane hwnd lookup failed error={error}"
                                    )),
                                },
                                None => logger.error("foreground change pane webview not found"),
                            }
                        } else {
                            logger.debug(format!(
                                "foreground change ignored mode={}",
                                state.0.load(Ordering::Relaxed)
                            ));
                        }
                    } else {
                        logger.debug("duplicate foreground event ignored");
                    }
                } else {
                    unsafe {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
            }

            let foreground_unhook_result = unsafe { UnhookWinEvent(foreground_hook) };
            let object_unhook_result = if object_hook.is_null() {
                0
            } else {
                unsafe { UnhookWinEvent(object_hook) }
            };
            EVENT_THREAD_ID.store(0, Ordering::Release);
            logger.info(format!(
                "window event hooks stopped foregroundResult={foreground_unhook_result} objectResult={object_unhook_result}"
            ));
        });
    }

    pub fn set_mode(
        window: &tauri::Window,
        state: &State,
        tray_state: &TrayState,
        logger: &Logger,
        mode: u8,
    ) -> Result<(), String> {
        let previous = state.0.load(Ordering::Relaxed);
        state.0.store(mode, Ordering::Relaxed);
        let pane = match window.hwnd() {
            Ok(raw) => raw.0 as Hwnd,
            Err(error) => {
                logger.error(format!("set_mode hwnd lookup failed error={error}"));
                return Err(error.to_string());
            }
        };
        logger.info(format!(
            "set_mode previous={previous} current={mode} pane={}",
            window_identity(pane)
        ));
        set_taskbar_visibility(pane, mode != 1, logger);
        if mode != 1 {
            tray_state.clear_auto_closed();
            set_owner(pane, std::ptr::null_mut(), logger);
            set_level(
                pane,
                if mode == 2 { -1isize } else { -2isize } as Hwnd,
                logger,
            );
            if previous == 1 && !tray_state.user_closed() {
                if let Err(error) = window.unminimize() {
                    logger.warn(format!("set_mode restore unminimize failed error={error}"));
                }
                if let Err(error) = window.show() {
                    logger.warn(format!("set_mode restore show failed error={error}"));
                }
            }
        } else {
            set_owner(pane, std::ptr::null_mut(), logger);
            set_level(pane, -2isize as Hwnd, logger);
            sync(
                pane,
                unsafe { GetForegroundWindow() },
                tray_state,
                logger,
                false,
            );
        }
        logger.debug(format!(
            "set_mode complete paneState=[{}]",
            pane_state(pane)
        ));
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::{
            foreground_changed, is_attached_codex_object, is_codex_executable_name,
            pane_visibility_action, PaneVisibilityAction, WinEventRecord, EVENT_OBJECT_HIDE,
            OBJID_WINDOW,
        };

        #[test]
        fn pane_visibility_distinguishes_auto_and_user_close() {
            assert_eq!(
                pane_visibility_action(false, false, false, true, true, false),
                PaneVisibilityAction::Show
            );
            assert_eq!(
                pane_visibility_action(false, false, false, false, true, false),
                PaneVisibilityAction::Keep
            );
            assert_eq!(
                pane_visibility_action(true, false, false, false, false, false),
                PaneVisibilityAction::Keep
            );
            assert_eq!(
                pane_visibility_action(false, false, true, false, true, false),
                PaneVisibilityAction::Keep
            );
            assert_eq!(
                pane_visibility_action(true, false, true, false, true, false),
                PaneVisibilityAction::Hide
            );
            assert_eq!(
                pane_visibility_action(true, false, false, false, true, false),
                PaneVisibilityAction::Keep
            );
            assert_eq!(
                pane_visibility_action(true, false, false, true, false, true),
                PaneVisibilityAction::Hide
            );
            assert_eq!(
                pane_visibility_action(false, false, false, true, false, true),
                PaneVisibilityAction::Keep
            );
        }

        #[test]
        fn recognizes_current_codex_host() {
            assert!(is_codex_executable_name("Codex.exe"));
            assert!(is_codex_executable_name("ChatGPT.exe"));
        }

        #[test]
        fn foreground_sync_runs_only_after_a_window_change() {
            let window = 1usize as super::Hwnd;
            assert!(foreground_changed(std::ptr::null_mut(), window));
            assert!(!foreground_changed(window, window));
        }

        #[test]
        fn object_sync_ignores_codex_child_windows() {
            let root = 1usize as super::Hwnd;
            let child = 2usize as super::Hwnd;
            let mut record = WinEventRecord {
                hook: std::ptr::null_mut(),
                event: EVENT_OBJECT_HIDE,
                hwnd: root,
                object: OBJID_WINDOW,
                child: 0,
                event_thread: 0,
                event_time: 0,
            };
            assert!(is_attached_codex_object(&record, root));
            record.hwnd = child;
            assert!(!is_attached_codex_object(&record, root));
            record.hwnd = root;
            record.object = 1;
            assert!(!is_attached_codex_object(&record, root));
        }
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod other {
    use super::TrayState;
    use crate::diagnostics::Logger;

    #[derive(Clone, Default)]
    pub struct State;
    pub fn start(_: tauri::AppHandle, _: State, _: TrayState, _: Logger) {}
    pub fn set_mode(
        window: &tauri::Window,
        _: &State,
        _: &TrayState,
        _: &Logger,
        mode: u8,
    ) -> Result<(), String> {
        window
            .set_always_on_top(mode == 2)
            .map_err(|error| error.to_string())
    }
}

#[cfg(not(windows))]
pub use other::*;
