use super::{effective_topmost, WindowTracking};
use std::{
    ffi::c_void,
    thread,
    time::{Duration, Instant},
};

type NativeWindow = *mut c_void;
type NativeHandle = *mut c_void;
type WindowCallback = Option<unsafe extern "system" fn(NativeWindow, isize) -> i32>;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const GW_OWNER: u32 = 4;
const GA_ROOT: u32 = 2;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_NOOWNERZORDER: u32 = 0x0200;
const MB_OKCANCEL: u32 = 0x0001;
const MB_ICONINFORMATION: u32 = 0x0040;
const MB_SETFOREGROUND: u32 = 0x0001_0000;
const IDOK: i32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[derive(Clone)]
struct WindowCandidate {
    handle: isize,
    process_id: u32,
    process_name: String,
}

struct WindowCollector {
    own_process_id: u32,
    windows: Vec<WindowCandidate>,
}

#[link(name = "user32")]
extern "system" {
    #[link_name = "EnumWindows"]
    fn enum_windows(callback: WindowCallback, value: isize) -> i32;
    #[link_name = "GetAncestor"]
    fn get_ancestor(window: NativeWindow, flags: u32) -> NativeWindow;
    #[link_name = "GetForegroundWindow"]
    fn get_foreground_window() -> NativeWindow;
    #[link_name = "GetWindow"]
    fn get_window(window: NativeWindow, command: u32) -> NativeWindow;
    #[link_name = "GetWindowRect"]
    fn get_window_rect(window: NativeWindow, rect: *mut NativeRect) -> i32;
    #[link_name = "GetWindowThreadProcessId"]
    fn get_window_thread_process_id(window: NativeWindow, process_id: *mut u32) -> u32;
    #[link_name = "IsIconic"]
    fn is_iconic(window: NativeWindow) -> i32;
    #[link_name = "IsWindow"]
    fn is_window(window: NativeWindow) -> i32;
    #[link_name = "IsWindowVisible"]
    fn is_window_visible(window: NativeWindow) -> i32;
    #[link_name = "MessageBoxW"]
    fn message_box(
        owner: NativeWindow,
        text: *const u16,
        caption: *const u16,
        kind: u32,
    ) -> i32;
    #[link_name = "SetWindowPos"]
    fn set_window_position(
        window: NativeWindow,
        insert_after: NativeWindow,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    #[link_name = "CloseHandle"]
    fn close_handle(handle: NativeHandle) -> i32;
    #[link_name = "OpenProcess"]
    fn open_process(access: u32, inherit_handle: i32, process_id: u32) -> NativeHandle;
    #[link_name = "QueryFullProcessImageNameW"]
    fn query_full_process_image_name(
        process: NativeHandle,
        flags: u32,
        filename: *mut u16,
        size: *mut u32,
    ) -> i32;
}

pub(super) fn start(own_window: isize, tracking: WindowTracking) {
    let _ = thread::Builder::new()
        .name("window-tracker".to_string())
        .spawn(move || {
            let own_process_id = process_id(own_window).unwrap_or_default();
            let mut next_search = Instant::now();

            loop {
                if !window_exists(own_window) {
                    break;
                }

                select_foreground_window(own_window, own_process_id, &tracking);
                validate_target(&tracking);

                let needs_target = tracking.lock().target.is_none();
                if needs_target && Instant::now() >= next_search {
                    find_preferred_window(own_window, own_process_id, &tracking);
                    next_search = Instant::now() + Duration::from_secs(1);
                }

                update_tracked_position(own_window, &tracking);
                update_topmost(own_window, &tracking);
                thread::sleep(Duration::from_millis(16));
            }
        });
}

pub(super) fn request_selection(own_window: isize, tracking: WindowTracking) {
    {
        let mut settings = tracking.lock();
        if settings.selection_prompt_open || settings.selection_armed {
            return;
        }
        settings.selection_prompt_open = true;
    }

    let _ = thread::Builder::new()
        .name("window-selection".to_string())
        .spawn(move || {
            let text = wide_string(
                "After closing this message, click the application window you want the timeline to follow.",
            );
            let caption = wide_string("Attach to window");
            let response = unsafe {
                message_box(
                    native_window(own_window),
                    text.as_ptr(),
                    caption.as_ptr(),
                    MB_OKCANCEL | MB_ICONINFORMATION | MB_SETFOREGROUND,
                )
            };
            let mut settings = tracking.lock();
            settings.selection_prompt_open = false;
            settings.selection_armed = response == IDOK;
        });
}

fn select_foreground_window(
    own_window: isize,
    own_process_id: u32,
    tracking: &WindowTracking,
) {
    if !tracking.lock().selection_armed {
        return;
    }

    let Some(handle) = foreground_root_window() else {
        return;
    };
    if handle == own_window || !selectable_window(handle, own_process_id) {
        return;
    }
    thread::sleep(Duration::from_millis(80));
    if foreground_root_window() != Some(handle) {
        return;
    }

    if let Some(candidate) = candidate_for_window(handle) {
        let mut settings = tracking.lock();
        if settings.selection_armed {
            attach(&mut settings, candidate, own_window);
            settings.selection_armed = false;
        }
    }
}

fn validate_target(tracking: &WindowTracking) {
    let target = tracking
        .lock()
        .target
        .as_ref()
        .map(|target| (target.handle, target.process_id));
    let Some((handle, expected_process_id)) = target else {
        return;
    };

    if !window_exists(handle) || process_id(handle) != Some(expected_process_id) {
        let mut settings = tracking.lock();
        if settings
            .target
            .as_ref()
            .is_some_and(|target| target.handle == handle)
        {
            settings.target = None;
            settings.attached_process = None;
            settings.actual_topmost = None;
        }
    }
}

fn find_preferred_window(own_window: isize, own_process_id: u32, tracking: &WindowTracking) {
    let preferred_process = tracking.lock().preferred_process.clone();
    let candidate = visible_windows(own_process_id)
        .into_iter()
        .find(|window| window.process_name.eq_ignore_ascii_case(&preferred_process));

    if let Some(candidate) = candidate {
        let mut settings = tracking.lock();
        if settings.target.is_none()
            && settings
                .preferred_process
                .eq_ignore_ascii_case(&preferred_process)
        {
            attach(&mut settings, candidate, own_window);
        }
    }
}

fn attach(
    settings: &mut super::WindowTrackingSettings,
    candidate: WindowCandidate,
    own_window: isize,
) {
    let Some(target_rect) = window_rect(candidate.handle) else {
        return;
    };
    let Some(own_rect) = window_rect(own_window) else {
        return;
    };

    let offset = (own_rect.left - target_rect.left, own_rect.top - target_rect.top);
    settings.preferred_process = candidate.process_name.clone();
    settings.attached_process = Some(candidate.process_name.clone());
    settings.target = Some(super::AttachedWindow {
        handle: candidate.handle,
        process_id: candidate.process_id,
        offset,
        last_target_position: (target_rect.left, target_rect.top),
        last_overlay_position: (own_rect.left, own_rect.top),
    });
    settings.actual_topmost = None;
}

fn update_tracked_position(own_window: isize, tracking: &WindowTracking) {
    let target = tracking.lock().target.as_ref().map(|target| {
        (
            target.handle,
            target.process_id,
            target.offset,
            target.last_target_position,
            target.last_overlay_position,
        )
    });
    let Some(target) = target else {
        return;
    };
    let (handle, process_id, offset, last_target_position, last_overlay_position) =
        target;
    if unsafe { is_iconic(native_window(handle)) } != 0 {
        return;
    }

    let Some(target_rect) = window_rect(handle) else {
        return;
    };
    let Some(own_rect) = window_rect(own_window) else {
        return;
    };
    let target_position = (target_rect.left, target_rect.top);
    let own_position = (own_rect.left, own_rect.top);

    let (new_offset, new_target_position, new_overlay_position) = if target_position
        != last_target_position
    {
        let desired_position = (
            target_position.0 + offset.0,
            target_position.1 + offset.1,
        );
        if own_position == desired_position
            || move_window(own_window, desired_position.0, desired_position.1)
        {
            (offset, target_position, desired_position)
        } else {
            (offset, last_target_position, last_overlay_position)
        }
    } else if own_position != last_overlay_position {
        (
            (
                own_position.0 - target_position.0,
                own_position.1 - target_position.1,
            ),
            target_position,
            own_position,
        )
    } else {
        (offset, target_position, own_position)
    };

    let mut settings = tracking.lock();
    if let Some(target) = settings
        .target
        .as_mut()
        .filter(|target| target.handle == handle && target.process_id == process_id)
    {
        target.offset = new_offset;
        target.last_target_position = new_target_position;
        target.last_overlay_position = new_overlay_position;
    }
}

fn update_topmost(own_window: isize, tracking: &WindowTracking) {
    let (always_on_top, hide_when_unfocused, target, actual_topmost) = {
        let settings = tracking.lock();
        (
            settings.always_on_top,
            settings.hide_when_unfocused,
            settings
                .target
                .as_ref()
                .map(|target| (target.handle, target.process_id)),
            settings.actual_topmost,
        )
    };
    let target_is_focused = target.is_some_and(|(handle, process_id)| {
        !is_minimized(handle) && foreground_process_id() == Some(process_id)
    });
    let should_be_topmost = effective_topmost(
        always_on_top,
        hide_when_unfocused,
        target_is_focused,
    );
    if actual_topmost == Some(should_be_topmost) {
        return;
    }

    if set_topmost(own_window, should_be_topmost) {
        tracking.lock().actual_topmost = Some(should_be_topmost);
    }
}

fn visible_windows(own_process_id: u32) -> Vec<WindowCandidate> {
    let mut collector = WindowCollector {
        own_process_id,
        windows: Vec::new(),
    };
    unsafe {
        enum_windows(
            Some(collect_window),
            (&mut collector as *mut WindowCollector) as isize,
        );
    }
    collector.windows
}

unsafe extern "system" fn collect_window(window: NativeWindow, value: isize) -> i32 {
    let collector = &mut *(value as *mut WindowCollector);
    let handle = window as isize;
    if selectable_window(handle, collector.own_process_id) {
        if let Some(candidate) = candidate_for_window(handle) {
            collector.windows.push(candidate);
        }
    }
    1
}

fn selectable_window(handle: isize, own_process_id: u32) -> bool {
    if !window_exists(handle)
        || unsafe { is_window_visible(native_window(handle)) } == 0
        || unsafe { is_iconic(native_window(handle)) } != 0
        || !(unsafe { get_window(native_window(handle), GW_OWNER) }).is_null()
    {
        return false;
    }
    process_id(handle).is_some_and(|process_id| process_id != own_process_id)
}

fn candidate_for_window(handle: isize) -> Option<WindowCandidate> {
    let process_id = process_id(handle)?;
    let process_name = process_name(process_id)?;
    Some(WindowCandidate {
        handle,
        process_id,
        process_name,
    })
}

fn process_id(handle: isize) -> Option<u32> {
    let mut process_id = 0;
    unsafe {
        get_window_thread_process_id(native_window(handle), &mut process_id);
    }
    (process_id != 0).then_some(process_id)
}

fn process_name(process_id: u32) -> Option<String> {
    let process = unsafe { open_process(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if process.is_null() {
        return None;
    }

    let mut path = vec![0_u16; 32_768];
    let mut length = path.len() as u32;
    let succeeded = unsafe {
        query_full_process_image_name(process, 0, path.as_mut_ptr(), &mut length) != 0
    };
    unsafe {
        close_handle(process);
    }
    if !succeeded || length == 0 {
        return None;
    }

    String::from_utf16_lossy(&path[..length as usize])
        .rsplit(['\\', '/'])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn foreground_process_id() -> Option<u32> {
    let foreground = unsafe { get_foreground_window() };
    (!foreground.is_null())
        .then(|| process_id(foreground as isize))
        .flatten()
}

fn foreground_root_window() -> Option<isize> {
    let foreground = unsafe { get_foreground_window() };
    if foreground.is_null() {
        return None;
    }
    let root = unsafe { get_ancestor(foreground, GA_ROOT) };
    Some((if root.is_null() { foreground } else { root }) as isize)
}

fn window_rect(handle: isize) -> Option<NativeRect> {
    let mut rect = NativeRect::default();
    let succeeded = unsafe { get_window_rect(native_window(handle), &mut rect) != 0 };
    succeeded.then_some(rect)
}

fn window_exists(handle: isize) -> bool {
    unsafe { is_window(native_window(handle)) != 0 }
}

fn is_minimized(handle: isize) -> bool {
    unsafe { is_iconic(native_window(handle)) != 0 }
}

fn move_window(handle: isize, x: i32, y: i32) -> bool {
    unsafe {
        set_window_position(
            native_window(handle),
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        ) != 0
    }
}

fn set_topmost(handle: isize, topmost: bool) -> bool {
    let insert_after = (if topmost { -1_isize } else { -2_isize }) as NativeWindow;
    unsafe {
        set_window_position(
            native_window(handle),
            insert_after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        ) != 0
    }
}

fn native_window(handle: isize) -> NativeWindow {
    handle as NativeWindow
}

fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
