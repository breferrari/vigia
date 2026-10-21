//! The terminal's one writer. Every byte the pane writes after the takeover
//! goes through it, on its own thread, so a terminal that reads slowly holds
//! this thread and never the event loop. `SPEC.md` §6, the terminal writer.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

/// Set by a panic's or a signal's restore: the writer writes nothing more, so
/// the restore is the last thing the terminal reads.
static POISONED: AtomicBool = AtomicBool::new(false);

/// Whether a write to the terminal is in flight, so a restore can wait it out.
static WRITING: AtomicBool = AtomicBool::new(false);

/// Stop every writer, and wait until no write of theirs is in flight, so the
/// restore that follows is the last thing the terminal reads. Bounded: a
/// write waits at most [`POLL`] for room.
pub fn poison() {
    POISONED.store(true, Ordering::SeqCst);
    // A panic on the writer's own thread would wait on itself.
    if std::thread::current().name() == Some(THREAD) {
        return;
    }
    let until = Instant::now() + 2 * POLL;
    while WRITING.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The image part sent, as [`image_bits`] packs it, or 0.
static OPEN: AtomicU64 = AtomicU64::new(0);

/// The last images whose upload began, as [`image_bits`] packs them, for a
/// restore that has no other record of them.
static SENT: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];

/// Where the next entry of [`SENT`] goes.
static SENT_NEXT: AtomicUsize = AtomicUsize::new(0);

/// An image id and whether its escapes pass through tmux, in one word, never 0.
fn image_bits(id: u32, tmux: bool) -> u64 {
    u64::from(id) | (u64::from(tmux) << 32) | (1 << 33)
}

/// [`image_bits`] undone.
fn image_of(bits: u64) -> Option<(u32, bool)> {
    (bits != 0).then_some((bits as u32, bits & (1 << 32) != 0))
}

/// The image left part sent, if one is: its id, and whether through tmux.
pub fn open_image() -> Option<(u32, bool)> {
    image_of(OPEN.load(Ordering::SeqCst))
}

/// The last images whose upload began, for a restore no delete reached.
pub fn sent_images() -> Vec<(u32, bool)> {
    SENT.iter()
        .filter_map(|bits| image_of(bits.load(Ordering::Acquire)))
        .collect()
}

/// A picture's transmit as the writer takes it: whole chunks, each a separate
/// escape, that no other graphics command may come between.
pub struct Upload {
    /// The image's id.
    pub id: u32,
    /// Every chunk, in order.
    pub data: Arc<str>,
    /// Where each chunk ends in `data`.
    pub ends: Arc<[usize]>,
    /// The empty last chunk that closes it if it is cut short.
    pub close: Arc<str>,
    /// Whether its escapes pass through tmux.
    pub tmux: bool,
    /// Marks the image held once its last chunk is written.
    pub ticket: crate::kitty::Ticket,
}

/// The longest one wait for room to write lasts, so a stop is noticed this soon.
const POLL: Duration = Duration::from_millis(50);

/// The writer thread's name.
const THREAD: &str = "vigia-writer";

/// How long a dropped writer goes on writing what it was handed.
const STOP_WAIT: Duration = Duration::from_millis(200);

/// The most one write hands a descriptor that may block: Linux reports room
/// while fewer than this many bytes are queued.
#[cfg(unix)]
const SHARED_PIECE: usize = 256;

/// The most a fresh frame buffer reserves, so one large picture does not set
/// the size of every buffer after it.
const BUFFER_CAP: usize = 64 * 1024;

/// The most one write hands the terminal, so a stop or a poison is noticed
/// between pieces of a large frame.
const PIECE: usize = 4096;

/// What the writer tells the loop.
pub struct Wakes {
    /// The bytes not yet written fell back to the threshold the loop asked about.
    pub cells: Box<dyn Fn() + Send>,
    /// A write failed, so nothing the pane draws reaches the terminal any more.
    pub lost: Box<dyn Fn() + Send>,
}

impl Wakes {
    /// Wakes nobody listens for.
    #[must_use]
    pub fn none() -> Self {
        Self {
            cells: Box::new(|| {}),
            lost: Box::new(|| {}),
        }
    }
}

/// State the loop and the writer share.
#[derive(Default)]
struct Shared {
    /// Bytes handed over and not yet written.
    unwritten: AtomicUsize,
    /// The backlog the loop last asked about.
    threshold: AtomicUsize,
    /// Whether the loop is waiting to hear the backlog fell to `threshold`.
    armed: AtomicBool,
    /// Whether to stop writing what is queued.
    abandon: AtomicBool,
    /// The size of the last frame handed over.
    last: AtomicUsize,
}

impl Shared {
    /// Count `written` bytes as gone, and wake the loop if it was waiting for
    /// the backlog to fall this far.
    fn wrote(&self, written: usize, wakes: &Wakes) {
        let left = self.unwritten.fetch_sub(written, Ordering::AcqRel) - written;
        if left <= self.threshold.load(Ordering::Acquire)
            && self.armed.swap(false, Ordering::AcqRel)
        {
            (wakes.cells)();
        }
    }

    fn stopping(&self) -> bool {
        self.abandon.load(Ordering::Acquire) || POISONED.load(Ordering::SeqCst)
    }
}

/// What the writer's queue carries.
enum Job {
    /// A frame's bytes: text, written ahead of any graphics waiting.
    Frame(Vec<u8>),
    /// An image to send, chunk by chunk, between frames.
    Upload(Upload),
    /// Delete image `id`: its upload is cancelled first, so the delete never
    /// lands inside its chunks.
    Delete { id: u32, bytes: Vec<u8> },
    /// The end `stop` queues, which a sink still open would otherwise keep
    /// from coming.
    Stop,
}

/// One entry of the graphics lane.
enum Graphic {
    /// An upload and how many of its chunks are written.
    Upload(Upload, usize),
    /// The close of an upload cut off, which stays the open image until written.
    Close(Vec<u8>),
    /// Bytes written whole.
    Raw(Vec<u8>),
}

/// What waits to be written: frames first, then graphics in order.
#[derive(Default)]
struct Lanes {
    text: VecDeque<Vec<u8>>,
    graphics: VecDeque<Graphic>,
    /// `stop` asked for the end: what is written next is all there is.
    stopping: bool,
}

impl Lanes {
    fn is_empty(&self) -> bool {
        self.text.is_empty() && self.graphics.is_empty()
    }

    fn take(&mut self, job: Job) {
        match job {
            Job::Frame(bytes) => self.text.push_back(bytes),
            Job::Upload(upload) if !self.stopping && !upload.ends.is_empty() => {
                self.graphics.push_back(Graphic::Upload(upload, 0));
            }
            Job::Upload(_) => {}
            Job::Delete { id, bytes } => {
                self.cancel(|of| of == id);
                self.graphics.push_back(Graphic::Raw(bytes));
            }
            Job::Stop => {
                self.stopping = true;
                self.cancel(|_| true);
            }
        }
    }

    /// Drop the uploads `which` names; one part sent is closed in its place.
    fn cancel(&mut self, which: impl Fn(u32) -> bool) {
        self.graphics.retain_mut(|graphic| {
            let Graphic::Upload(upload, sent) = graphic else {
                return true;
            };
            if !which(upload.id) {
                return true;
            }
            if *sent == 0 {
                return false;
            }
            let close = upload.close.as_bytes().to_vec();
            *graphic = Graphic::Close(close);
            true
        });
    }

    /// Write the next thing due: a frame, else one graphics chunk whole.
    /// `Ok(false)` once there is nothing left to write.
    fn write_next(
        &mut self,
        out: &mut impl Out,
        shared: &Shared,
        wakes: &Wakes,
    ) -> io::Result<bool> {
        if let Some(frame) = self.text.pop_front() {
            write_out(out, &frame, shared, &mut |written| {
                shared.wrote(written, wakes)
            })?;
            return Ok(true);
        }
        let Some(graphic) = self.graphics.front_mut() else {
            return Ok(false);
        };
        match graphic {
            Graphic::Raw(bytes) => {
                write_out(out, bytes, shared, &mut |_| {})?;
                self.graphics.pop_front();
            }
            Graphic::Close(bytes) => {
                write_out(out, bytes, shared, &mut |_| {})?;
                if !shared.stopping() {
                    OPEN.store(0, Ordering::SeqCst);
                }
                self.graphics.pop_front();
            }
            Graphic::Upload(upload, sent) => {
                if *sent == 0 {
                    let bits = image_bits(upload.id, upload.tmux);
                    OPEN.store(bits, Ordering::SeqCst);
                    let slot = SENT_NEXT.fetch_add(1, Ordering::AcqRel) % SENT.len();
                    SENT[slot].store(bits, Ordering::Release);
                }
                let from = sent.checked_sub(1).map_or(0, |last| upload.ends[last]);
                let to = upload.ends[*sent];
                write_out(out, &upload.data.as_bytes()[from..to], shared, &mut |_| {})?;
                *sent += 1;
                if *sent == upload.ends.len() && !shared.stopping() {
                    OPEN.store(0, Ordering::SeqCst);
                    upload.ticket.written();
                    // A redraw, so its cells are drawn.
                    (wakes.cells)();
                    self.graphics.pop_front();
                }
            }
        }
        Ok(true)
    }
}

/// The thread that writes the terminal, and the handle the loop draws through.
pub struct Writer {
    queue: Option<mpsc::Sender<Job>>,
    shared: Arc<Shared>,
    finished: mpsc::Receiver<()>,
}

impl Writer {
    /// Start writing to `out` on a thread of its own.
    pub fn start(out: impl Out + Send + 'static, wakes: Wakes) -> Self {
        let shared = Arc::new(Shared::default());
        let (queue, jobs) = mpsc::channel::<Job>();
        let (done, finished) = mpsc::channel();
        let theirs = Arc::clone(&shared);
        let _ = std::thread::Builder::new()
            .name(THREAD.to_owned())
            .spawn(move || {
                let mut out = out;
                let mut lanes = Lanes::default();
                loop {
                    // Everything that has arrived is sorted first, so a frame
                    // queued behind an upload goes ahead of its next chunk.
                    let job = if lanes.is_empty() {
                        if lanes.stopping {
                            break;
                        }
                        match jobs.recv() {
                            Ok(job) => Some(job),
                            Err(_) => break,
                        }
                    } else {
                        jobs.try_recv().ok()
                    };
                    if let Some(job) = job {
                        lanes.take(job);
                        continue;
                    }
                    if theirs.stopping() {
                        break;
                    }
                    if lanes.write_next(&mut out, &theirs, &wakes).is_err() {
                        (wakes.lost)();
                        break;
                    }
                }
                let _ = done.send(());
            });
        Self {
            queue: Some(queue),
            shared,
            finished,
        }
    }

    /// What ratatui's backend writes into. Its bytes go to the writer on flush.
    pub fn sink(&self) -> Sink {
        Sink {
            queue: self.queue.clone(),
            shared: Arc::clone(&self.shared),
            pending: Vec::new(),
        }
    }

    /// The size of the last frame handed over.
    pub fn last_frame(&self) -> usize {
        self.shared.last.load(Ordering::Acquire)
    }

    /// Bytes handed over and not yet written.
    pub fn unwritten(&self) -> usize {
        self.shared.unwritten.load(Ordering::Acquire)
    }

    /// Whether more than `threshold` bytes wait. When so, the loop is woken
    /// once they fall to it. Re-read after arming, so a drain between the
    /// check and the arming is not lost.
    pub fn backlogged(&self, threshold: usize) -> bool {
        self.shared.threshold.store(threshold, Ordering::Release);
        if self.unwritten() <= threshold {
            return false;
        }
        self.shared.armed.store(true, Ordering::Release);
        if self.unwritten() <= threshold {
            self.shared.armed.store(false, Ordering::Release);
            return false;
        }
        true
    }

    /// Wait up to `limit` for everything handed over to be written.
    pub fn drain(&self, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        while self.unwritten() > 0 {
            if Instant::now() >= until || self.shared.stopping() {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }

    /// Write what is queued for up to `limit`, then drop the rest and stop.
    /// Returns once the thread has stopped, or after `limit` and one more
    /// wait if a write it cannot leave is in flight: whether it stopped.
    pub fn stop(&mut self, limit: Duration) -> bool {
        if let Some(queue) = self.queue.take() {
            let _ = queue.send(Job::Stop);
        }
        // A thread already gone has dropped its end, which is as good as done.
        let ended = |waited| !matches!(waited, Err(mpsc::RecvTimeoutError::Timeout));
        if ended(self.finished.recv_timeout(limit)) {
            return true;
        }
        self.shared.abandon.store(true, Ordering::Release);
        ended(self.finished.recv_timeout(POLL * 2))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.stop(STOP_WAIT);
    }
}

/// The writer's end of ratatui's backend: bytes gather until a flush, which
/// hands them to the writer whole.
pub struct Sink {
    queue: Option<mpsc::Sender<Job>>,
    shared: Arc<Shared>,
    pending: Vec<u8>,
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.hand_over(true)
    }
}

impl Sink {
    /// Hand `bytes` over after what is pending, as something other than a
    /// frame, so the skip threshold keeps the last frame's size.
    ///
    /// # Errors
    ///
    /// The writer has stopped.
    pub fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.pending.extend_from_slice(bytes);
        self.hand_over(false)
    }

    /// Queue `upload` on the graphics lane.
    ///
    /// # Errors
    ///
    /// The writer has stopped.
    pub fn upload(&mut self, upload: Upload) -> io::Result<()> {
        self.job(Job::Upload(upload))
    }

    /// Delete image `id` with `bytes`, after cancelling its upload: what is
    /// not written is dropped, and a part sent is closed.
    ///
    /// # Errors
    ///
    /// The writer has stopped.
    pub fn delete(&mut self, id: u32, bytes: Vec<u8>) -> io::Result<()> {
        self.job(Job::Delete { id, bytes })
    }

    fn job(&mut self, job: Job) -> io::Result<()> {
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| io::Error::from(io::ErrorKind::BrokenPipe))?;
        queue
            .send(job)
            .map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))
    }

    fn hand_over(&mut self, frame: bool) -> io::Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let size = if frame {
            self.shared.last.load(Ordering::Acquire).min(BUFFER_CAP)
        } else {
            0
        };
        let bytes = std::mem::replace(&mut self.pending, Vec::with_capacity(size));
        let Some(queue) = self.queue.as_ref() else {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        };
        self.shared
            .unwritten
            .fetch_add(bytes.len(), Ordering::AcqRel);
        if frame {
            self.shared.last.store(bytes.len(), Ordering::Release);
        }
        let len = bytes.len();
        queue.send(Job::Frame(bytes)).map_err(|_| {
            self.shared.unwritten.fetch_sub(len, Ordering::AcqRel);
            io::Error::from(io::ErrorKind::BrokenPipe)
        })
    }
}

/// Something the writer can write the terminal through.
pub trait Out {
    /// Write some of `bytes`, waiting at most `wait` for room. `Ok(0)` means
    /// there was no room in that time.
    ///
    /// # Errors
    ///
    /// The terminal refused the write.
    fn write_some(&mut self, bytes: &[u8], wait: Duration) -> io::Result<usize>;
}

/// Write all of `bytes` in pieces, counting each as it goes, unless told to stop.
fn write_out(
    out: &mut impl Out,
    bytes: &[u8],
    shared: &Shared,
    count: &mut dyn FnMut(usize),
) -> io::Result<()> {
    let mut rest = bytes;
    while !rest.is_empty() {
        // Marked before the check, so `poison` either stops this write or waits for it.
        WRITING.store(true, Ordering::SeqCst);
        if shared.stopping() {
            WRITING.store(false, Ordering::SeqCst);
            count(rest.len());
            return Ok(());
        }
        let piece = &rest[..rest.len().min(PIECE)];
        let written = out.write_some(piece, POLL);
        WRITING.store(false, Ordering::SeqCst);
        let written = written?;
        if written > 0 {
            count(written);
            rest = &rest[written..];
        }
    }
    Ok(())
}

/// The terminal standard output names, opened again for this process alone.
#[cfg(unix)]
pub struct Tty {
    file: std::fs::File,
    /// Standard output itself, because its device could not be opened again.
    /// Its open file is the shell's, so its blocking flag is left as it is.
    blocking: bool,
}

#[cfg(unix)]
impl Tty {
    /// Open the device standard output is, nonblocking. Not `/dev/tty`, which
    /// macOS polls as `POLLNVAL`, and not a `dup`, which would share the flag
    /// with the shell's own output. Standard output itself, blocking, when the
    /// device cannot be named or opened again.
    ///
    /// # Errors
    ///
    /// Standard output cannot be duplicated.
    pub fn open() -> io::Result<Self> {
        use std::os::fd::AsFd;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;
        let file = rustix::termios::ttyname(std::io::stdout(), Vec::new())
            .map_err(io::Error::from)
            .and_then(|name| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(
                        rustix::fs::OFlags::NONBLOCK.bits() as i32
                            | rustix::fs::OFlags::NOCTTY.bits() as i32,
                    )
                    .open(std::ffi::OsStr::from_bytes(name.as_bytes()))
            });
        match file {
            Ok(file) => Ok(Self {
                file,
                blocking: false,
            }),
            // Another user's terminal under `su` refuses the open, and a
            // container may have no device by that name.
            Err(_) => Ok(Self {
                file: std::fs::File::from(std::io::stdout().as_fd().try_clone_to_owned()?),
                blocking: true,
            }),
        }
    }
}

#[cfg(unix)]
impl Out for Tty {
    fn write_some(&mut self, bytes: &[u8], wait: Duration) -> io::Result<usize> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        // A shared descriptor may block. `POLLOUT` promises only some room, so
        // a small piece keeps a block short; nothing bounds it.
        let bytes = if self.blocking {
            &bytes[..bytes.len().min(SHARED_PIECE)]
        } else {
            bytes
        };
        let mut fds = [PollFd::new(&self.file, PollFlags::OUT)];
        let timeout = Timespec::try_from(wait.min(POLL)).expect("under a second");
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) => return Ok(0),
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => return Ok(0),
            Err(e) => return Err(e.into()),
        }
        // A descriptor `poll` cannot watch is an error, not a wait: retried, it
        // would spin, and written blocking, it would hold the thread again.
        let events = fds[0].revents();
        if events.intersects(PollFlags::NVAL | PollFlags::ERR | PollFlags::HUP) {
            return Err(io::Error::other(format!(
                "the terminal cannot be written: {events:?}"
            )));
        }
        match self.file.write(bytes) {
            Ok(written) => Ok(written),
            Err(e)
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::Interrupted =>
            {
                Ok(0)
            }
            Err(e) => Err(e),
        }
    }
}

/// Standard output, written blocking on the writer's thread. Windows has no
/// `ttyname` and no `poll` on a console, so a write in flight cannot be left.
#[cfg(windows)]
pub struct Tty {
    out: std::io::Stdout,
}

#[cfg(windows)]
impl Tty {
    /// The standard output handle.
    ///
    /// # Errors
    ///
    /// Never; the signature matches Unix.
    pub fn open() -> io::Result<Self> {
        Ok(Self {
            out: std::io::stdout(),
        })
    }
}

#[cfg(windows)]
impl Out for Tty {
    fn write_some(&mut self, bytes: &[u8], _wait: Duration) -> io::Result<usize> {
        let written = self.out.write(bytes)?;
        self.out.flush()?;
        Ok(written)
    }
}

/// A descriptor the restore writes through: the terminal opened again, every
/// write given up after its deadline, so a terminal that stopped reading
/// cannot hold the exit.
pub struct Restore {
    #[cfg(unix)]
    tty: Option<Tty>,
    #[cfg(windows)]
    console: Option<std::fs::File>,
    until: Instant,
}

impl Restore {
    /// Open it, with everything written through it due within `limit`.
    pub fn open(limit: Duration) -> Self {
        Self {
            #[cfg(unix)]
            tty: Tty::open().ok(),
            #[cfg(windows)]
            console: std::fs::OpenOptions::new().write(true).open("CONOUT$").ok(),
            until: Instant::now() + limit,
        }
    }

    /// Give what is written from now on `limit` to arrive.
    pub fn arm(&mut self, limit: Duration) {
        self.until = Instant::now() + limit;
    }
}

/// Ends an escape string a frame left half written, so the bytes after it
/// are read as themselves. kitty ends an APC only on this, not on CAN.
pub const STRING_END: &[u8] = b"\x1b\\";

impl Write for Restore {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            let Some(tty) = self.tty.as_mut() else {
                return std::io::stdout().write(buf);
            };
            loop {
                let left = self.until.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    // Dropped rather than failed, so the steps after this one are
                    // still tried: raw mode goes back whatever the screen does.
                    return Ok(buf.len());
                }
                let written = tty.write_some(buf, left.min(Duration::from_millis(20)))?;
                if written > 0 {
                    return Ok(written);
                }
            }
        }
        #[cfg(windows)]
        {
            match self.console.as_mut() {
                Some(console) => console.write(buf),
                None => std::io::stdout().write(buf),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// An `Out` that takes what it is told to, and records it.
    #[derive(Clone, Default)]
    struct Taken {
        bytes: Arc<Mutex<Vec<u8>>>,
        room: Arc<AtomicUsize>,
    }

    impl Out for Taken {
        fn write_some(&mut self, bytes: &[u8], wait: Duration) -> io::Result<usize> {
            let room = self.room.load(Ordering::Acquire);
            if room == 0 {
                std::thread::sleep(wait.min(Duration::from_millis(2)));
                return Ok(0);
            }
            let n = bytes.len().min(room);
            self.room.fetch_sub(n, Ordering::AcqRel);
            self.bytes
                .lock()
                .expect("bytes")
                .extend_from_slice(&bytes[..n]);
            Ok(n)
        }
    }

    #[test]
    fn writer_order() {
        let out = Taken::default();
        out.room.store(usize::MAX, Ordering::Release);
        let writer = Writer::start(out.clone(), Wakes::none());
        let mut sink = writer.sink();
        let mut sent = Vec::new();
        for n in 0..200u32 {
            let frame = format!("frame {n} ").repeat(n as usize % 50 + 1);
            sink.write_all(frame.as_bytes()).expect("write");
            sink.flush().expect("flush");
            sent.extend_from_slice(frame.as_bytes());
        }
        assert!(
            writer.drain(Duration::from_secs(5)),
            "the frames were not written"
        );
        assert_eq!(
            *out.bytes.lock().expect("bytes"),
            sent,
            "frames came out of order or torn"
        );
    }

    #[test]
    fn backlog_wakes_once() {
        let out = Taken::default();
        let woken = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&woken);
        let writer = Writer::start(
            out.clone(),
            Wakes {
                cells: Box::new(move || {
                    count.fetch_add(1, Ordering::AcqRel);
                }),
                lost: Box::new(|| {}),
            },
        );
        let mut sink = writer.sink();
        sink.write_all(&[b'x'; 10_000]).expect("write");
        sink.flush().expect("flush");
        assert!(
            writer.backlogged(4096),
            "a held terminal's backlog was not seen"
        );
        // The terminal takes everything: one wake, no more.
        out.room.store(usize::MAX, Ordering::Release);
        assert!(writer.drain(Duration::from_secs(5)), "never drained");
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(
            woken.load(Ordering::Acquire),
            1,
            "the loop was not woken exactly once"
        );
        assert!(
            !writer.backlogged(4096),
            "a drained writer still reads as backlogged"
        );
    }

    #[test]
    fn stop_bounded() {
        // A terminal that never reads: the stop gives up within its limit.
        let out = Taken::default();
        let mut writer = Writer::start(out, Wakes::none());
        let mut sink = writer.sink();
        sink.write_all(&[b'x'; 100_000]).expect("write");
        sink.flush().expect("flush");
        let began = Instant::now();
        let finished = writer.stop(Duration::from_millis(100));
        assert!(
            finished,
            "a stop against a terminal that never reads left its thread running"
        );
        // A hang guard only: the stop's own work is the thread ending above.
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "a stop against a terminal that never reads took {:?}",
            began.elapsed()
        );
    }

    #[test]
    fn stop_open_sink() {
        // Everything written and a sink still open: the stop ends the thread
        // at once rather than waiting out its limit for a frame that never comes.
        let out = Taken::default();
        out.room.store(usize::MAX, Ordering::Release);
        let mut writer = Writer::start(out, Wakes::none());
        let mut sink = writer.sink();
        sink.write_all(b"frame").expect("write");
        sink.flush().expect("flush");
        assert!(
            writer.drain(Duration::from_secs(2)),
            "the frame was not written"
        );
        let began = Instant::now();
        assert!(
            writer.stop(Duration::from_secs(5)),
            "the thread did not stop"
        );
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "with a sink open the stop waited {:?}, its limit rather than the thread",
            began.elapsed()
        );
        drop(sink);
    }

    /// An upload of three chunks, each its own escape.
    fn three_chunks(id: u32) -> Upload {
        let first = format!("\x1b_Gi={id},m=1;AAAA\x1b\\");
        let chunks = [
            first.as_str(),
            "\x1b_Gm=1;BBBB\x1b\\",
            "\x1b_Gm=0;CCCC\x1b\\",
        ];
        let mut ends = Vec::new();
        let mut at = 0;
        for chunk in chunks {
            at += chunk.len();
            ends.push(at);
        }
        Upload {
            id,
            data: chunks.concat().into(),
            ends: ends.into(),
            close: "\x1b_Gm=0,q=2;\x1b\\".into(),
            tmux: false,
            ticket: crate::kitty::ticket().0,
        }
    }

    /// Wait up to 5 s for what `out` took to satisfy `done`.
    fn wait_for(out: &Taken, done: impl Fn(&str) -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !done(&written(out)) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Wait until `out` has taken `len` bytes.
    fn taken_len(out: &Taken, len: usize) {
        wait_for(out, |taken| taken.len() >= len);
        assert_eq!(
            out.bytes.lock().expect("bytes").len(),
            len,
            "the writer never took {len} bytes"
        );
    }

    fn written(out: &Taken) -> String {
        String::from_utf8(out.bytes.lock().expect("bytes").clone()).expect("utf8")
    }

    #[test]
    fn lane_order() {
        // Text first; a chunk written whole; text only between chunks; a
        // delete queued behind an image after its last chunk; the upload
        // reported once whole.
        let out = Taken::default();
        let (tx, rx) = mpsc::channel();
        let writer = Writer::start(
            out.clone(),
            Wakes {
                cells: Box::new(move || {
                    let _ = tx.send(());
                }),
                ..Wakes::none()
            },
        );
        let mut sink = writer.sink();
        let (ticket, held) = crate::kitty::ticket();
        let upload = Upload {
            ticket,
            ..three_chunks(7)
        };
        let first = upload.ends[0];
        sink.upload(upload).expect("upload");
        sink.delete(8, b"<delete>".to_vec()).expect("delete");
        sink.write_all(b"<frame1>").expect("write");
        sink.flush().expect("flush");
        // Room for the frame and the first chunk, then none: the second chunk waits.
        out.room.store(8 + first, Ordering::Release);
        taken_len(&out, 8 + first);
        sink.write_all(b"<frame2>").expect("write");
        sink.flush().expect("flush");
        std::thread::sleep(Duration::from_millis(20));
        out.room.store(usize::MAX, Ordering::Release);
        assert!(
            writer.drain(Duration::from_secs(5)),
            "the frames were not written"
        );
        wait_for(&out, |taken| taken.ends_with("<delete>"));
        assert_eq!(
            written(&out),
            "<frame1>\x1b_Gi=7,m=1;AAAA\x1b\\\x1b_Gm=1;BBBB\x1b\\<frame2>\x1b_Gm=0;CCCC\x1b\\<delete>",
            "the lanes were not written in their order"
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)),
            Ok(()),
            "the upload was not reported"
        );
        assert!(
            held(),
            "the upload written whole did not mark its image held"
        );
    }

    #[test]
    fn cancel_open() {
        // Cancelled part sent: the chunk being written finishes, the rest goes,
        // the close and then the delete follow, and no upload is reported.
        let out = Taken::default();
        let (tx, rx) = mpsc::channel();
        let writer = Writer::start(
            out.clone(),
            Wakes {
                cells: Box::new(move || {
                    let _ = tx.send(());
                }),
                ..Wakes::none()
            },
        );
        let mut sink = writer.sink();
        let upload = three_chunks(9);
        let first = upload.ends[0];
        sink.upload(upload).expect("upload");
        out.room.store(first, Ordering::Release);
        taken_len(&out, first);
        sink.delete(9, b"<delete>".to_vec()).expect("delete");
        std::thread::sleep(Duration::from_millis(20));
        out.room.store(usize::MAX, Ordering::Release);
        wait_for(&out, |taken| taken.ends_with("<delete>"));
        assert_eq!(
            written(&out),
            "\x1b_Gi=9,m=1;AAAA\x1b\\\x1b_Gm=1;BBBB\x1b\\\x1b_Gm=0,q=2;\x1b\\<delete>",
            "a cancelled upload was not closed, then deleted"
        );
        assert!(
            rx.try_recv().is_err(),
            "a cancelled upload was reported whole"
        );
    }

    #[test]
    fn chunk_whole() {
        // A chunk longer than one write's piece is still written whole before
        // a frame that arrives while it is going out.
        let out = Taken::default();
        let writer = Writer::start(out.clone(), Wakes::none());
        let mut sink = writer.sink();
        let chunk = format!("\x1b_Gi=5,m=0;{}\x1b\\", "A".repeat(6000));
        let len = chunk.len();
        sink.upload(Upload {
            id: 5,
            data: chunk.clone().into(),
            ends: vec![len].into(),
            close: "".into(),
            tmux: false,
            ticket: crate::kitty::ticket().0,
        })
        .expect("upload");
        out.room.store(100, Ordering::Release);
        taken_len(&out, 100);
        sink.write_all(b"<frame>").expect("write");
        sink.flush().expect("flush");
        std::thread::sleep(Duration::from_millis(20));
        out.room.store(usize::MAX, Ordering::Release);
        wait_for(&out, |taken| taken.ends_with("<frame>"));
        assert_eq!(
            written(&out),
            format!("{chunk}<frame>"),
            "a frame cut into a chunk"
        );
    }

    #[test]
    fn cancel_last_chunk() {
        // A delete that comes while the last chunk is going out follows it: the
        // image completes, then goes.
        let out = Taken::default();
        let (tx, rx) = mpsc::channel();
        let writer = Writer::start(
            out.clone(),
            Wakes {
                cells: Box::new(move || {
                    let _ = tx.send(());
                }),
                ..Wakes::none()
            },
        );
        let mut sink = writer.sink();
        let upload = three_chunks(13);
        let data = upload.data.to_string();
        let into_last = upload.ends[1] + 2;
        sink.upload(upload).expect("upload");
        out.room.store(into_last, Ordering::Release);
        taken_len(&out, into_last);
        sink.delete(13, b"<delete>".to_vec()).expect("delete");
        std::thread::sleep(Duration::from_millis(20));
        out.room.store(usize::MAX, Ordering::Release);
        wait_for(&out, |taken| taken.ends_with("<delete>"));
        assert_eq!(
            written(&out),
            format!("{data}<delete>"),
            "a delete cut into the last chunk"
        );
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)),
            Ok(()),
            "the whole upload was not reported"
        );
    }

    #[test]
    fn cancel_queued() {
        // Cancelled before it starts: none of it is written.
        let out = Taken::default();
        let writer = Writer::start(out.clone(), Wakes::none());
        let mut sink = writer.sink();
        sink.write_all(b"<frame>").expect("write");
        sink.flush().expect("flush");
        // The writer holds the frame while these arrive.
        std::thread::sleep(Duration::from_millis(20));
        sink.upload(three_chunks(11)).expect("upload");
        sink.delete(11, b"<delete>".to_vec()).expect("delete");
        out.room.store(usize::MAX, Ordering::Release);
        wait_for(&out, |taken| taken.ends_with("<delete>"));
        assert_eq!(
            written(&out),
            "<frame><delete>",
            "a cancelled upload was written"
        );
    }

    #[test]
    fn empty_frame_kept() {
        // A frame with no bytes is a frame, not a stop: the writer goes on.
        let out = Taken::default();
        out.room.store(usize::MAX, Ordering::Release);
        let writer = Writer::start(out.clone(), Wakes::none());
        writer
            .queue
            .as_ref()
            .expect("running")
            .send(Job::Frame(Vec::new()))
            .expect("queue");
        let mut sink = writer.sink();
        sink.write_all(b"after").expect("write");
        sink.flush().expect("flush");
        assert!(
            writer.drain(Duration::from_secs(2)),
            "the writer stopped at an empty frame"
        );
        assert_eq!(out.bytes.lock().expect("bytes").as_slice(), b"after");
    }

    #[test]
    fn drop_bounded() {
        let writer = Writer::start(Taken::default(), Wakes::none());
        let mut sink = writer.sink();
        sink.write_all(&[b'x'; 9000]).expect("write");
        sink.flush().expect("flush");
        drop(sink);
        let began = Instant::now();
        drop(writer);
        assert!(
            began.elapsed() < Duration::from_millis(500),
            "dropping a writer whose terminal never reads took {:?}",
            began.elapsed()
        );
    }

    #[test]
    fn send_not_frame() {
        let out = Taken::default();
        let writer = Writer::start(out, Wakes::none());
        let mut sink = writer.sink();
        sink.write_all(&[b'x'; 9000]).expect("write");
        sink.flush().expect("flush");
        sink.send(b"\x1b]52;c;eA==\x07").expect("send");
        assert_eq!(
            writer.last_frame(),
            9000,
            "an escape sent outside a frame was taken for the last frame's size"
        );
    }

    #[test]
    fn lost_on_error() {
        struct Broken;
        impl Out for Broken {
            fn write_some(&mut self, _: &[u8], _: Duration) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::BrokenPipe))
            }
        }
        let (tx, rx) = mpsc::channel();
        let writer = Writer::start(
            Broken,
            Wakes {
                cells: Box::new(|| {}),
                lost: Box::new(move || {
                    let _ = tx.send(());
                }),
            },
        );
        let mut sink = writer.sink();
        sink.write_all(b"frame").expect("write");
        sink.flush().expect("flush");
        assert!(
            rx.recv_timeout(Duration::from_secs(2)).is_ok(),
            "a failed write was not reported"
        );
    }
}
