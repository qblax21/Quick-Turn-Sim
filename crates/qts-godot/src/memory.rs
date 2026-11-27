// SPDX-License-Identifier: Apache-2.0
//! What the game process holds in memory, for the render bench and the
//! memory budget check (D-128): its working set and the video memory
//! Windows counts against it.
//!
//! Godot reports its own renderer's allocations; these are the operating
//! system's and the driver's counts, which include what Godot does not see
//! (the driver's own share, shader caches, the swap chain).

/// Bytes of the process's memory, as Windows counts them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ProcessMemory {
    /// Working set now.
    pub working_set: u64,
    /// Largest working set so far.
    pub peak_working_set: u64,
    /// Private bytes committed.
    pub private: u64,
    /// Dedicated video memory (on the GPU) Windows counts against this
    /// process, summed over adapters, as Task Manager shows it; 0 where
    /// there is no such count (a software renderer, another OS).
    pub video_local: u64,
    /// Shared video memory (system RAM the GPU maps) counted against it.
    pub video_non_local: u64,
}

#[cfg(windows)]
pub fn process_memory() -> ProcessMemory {
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut out = ProcessMemory::default();
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    // SAFETY: the pointer is to a local of the size passed; the pseudo
    // handle of the current process needs no closing.
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            std::ptr::addr_of_mut!(counters).cast(),
            counters.cb,
        )
    };
    if ok.as_bool() {
        out.working_set = counters.WorkingSetSize as u64;
        out.peak_working_set = counters.PeakWorkingSetSize as u64;
        out.private = counters.PrivateUsage as u64;
    }
    let (local, non_local) = video_memory();
    out.video_local = local;
    out.video_non_local = non_local;
    out
}

/// The latest video memory reading, from a thread of its own (D-130). The
/// Windows counters take about 200 ms to open on first use and can block
/// for longer, so reading them on the caller's thread stalled the frame
/// that asked: the first call starts the thread and returns zeros, later
/// calls what it last read (every half second).
#[cfg(windows)]
fn video_memory() -> (u64, u64) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Once;
    static LOCAL: AtomicU64 = AtomicU64::new(0);
    static NON_LOCAL: AtomicU64 = AtomicU64::new(0);
    static START: Once = Once::new();
    START.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("qts-video-memory".into())
            .spawn(|| loop {
                LOCAL.store(gpu_process_memory("Dedicated Usage"), Ordering::Relaxed);
                NON_LOCAL.store(gpu_process_memory("Shared Usage"), Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_millis(500));
            });
    });
    (LOCAL.load(Ordering::Relaxed), NON_LOCAL.load(Ordering::Relaxed))
}

/// This process's video memory from the Windows "GPU Process Memory"
/// counters (what Task Manager shows), summed over adapters: `counter` is
/// "Dedicated Usage" (on the GPU) or "Shared Usage" (system RAM it maps).
/// 0 if the counters are missing, as with a software renderer.
#[cfg(windows)]
fn gpu_process_memory(counter: &str) -> u64 {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Performance::{
        PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_LARGE, PDH_MORE_DATA,
    };
    let path = HSTRING::from(format!(
        "\\GPU Process Memory(pid_{}_*)\\{counter}",
        std::process::id()
    ));
    let mut total = 0u64;
    // SAFETY: the query and counter handles are used only here and the
    // query is closed on every path; the item buffer is sized as PDH asks.
    unsafe {
        let mut query = 0isize;
        if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
            return 0;
        }
        let mut handle = 0isize;
        if PdhAddEnglishCounterW(query, &path, 0, &mut handle) == 0
            && PdhCollectQueryData(query) == 0
        {
            let (mut size, mut count) = (0u32, 0u32);
            if PdhGetFormattedCounterArrayW(handle, PDH_FMT_LARGE, &mut size, &mut count, None)
                == PDH_MORE_DATA
            {
                let n =
                    (size as usize).div_ceil(std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>());
                let mut items = vec![PDH_FMT_COUNTERVALUE_ITEM_W::default(); n.max(1)];
                if PdhGetFormattedCounterArrayW(
                    handle,
                    PDH_FMT_LARGE,
                    &mut size,
                    &mut count,
                    Some(items.as_mut_ptr()),
                ) == 0
                {
                    for item in items.iter().take(count as usize) {
                        total += item.FmtValue.Anonymous.largeValue.max(0) as u64;
                    }
                }
            }
        }
        PdhCloseQuery(query);
    }
    total
}

#[cfg(not(windows))]
pub fn process_memory() -> ProcessMemory {
    ProcessMemory::default()
}
