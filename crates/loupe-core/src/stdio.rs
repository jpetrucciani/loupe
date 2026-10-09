//! Cancellable Unix I/O without Tokio's blocking standard-input worker.

use rustix::fs::{FileType, OFlags, fcntl_getfl, fcntl_setfl, fstat};
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use tokio::io::unix::AsyncFd;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

struct Descriptor {
    fd: OwnedFd,
    original: OFlags,
}

impl AsRawFd for Descriptor {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // Duplication shares status flags with the caller's descriptor.
        let _ = fcntl_setfl(&self.fd, self.original);
    }
}

enum Handle {
    Ready(AsyncFd<Descriptor>),
    File(Descriptor),
}

pub struct Stream {
    handle: Handle,
}

impl Stream {
    pub fn new(source: impl AsFd) -> io::Result<Self> {
        let fd = rustix::io::fcntl_dupfd_cloexec(source, 3)?;
        let original = fcntl_getfl(&fd)?;
        let descriptor = Descriptor { fd, original };
        let regular =
            FileType::from_raw_mode(fstat(&descriptor.fd)?.st_mode) == FileType::RegularFile;
        fcntl_setfl(&descriptor.fd, original | OFlags::NONBLOCK)?;
        let handle = if regular {
            // Regular files cannot be registered with epoll/kqueue.
            Handle::File(descriptor)
        } else {
            Handle::Ready(AsyncFd::new(descriptor)?)
        };
        Ok(Self { handle })
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        loop {
            let read = match &self.handle {
                Handle::Ready(handle) => {
                    let mut readiness = ready!(handle.poll_read_ready(context))?;
                    match readiness.try_io(|handle| {
                        rustix::io::read(&handle.get_ref().fd, buffer.initialize_unfilled())
                            .map_err(io::Error::from)
                    }) {
                        Ok(result) => result,
                        Err(_) => continue,
                    }
                }
                Handle::File(descriptor) => {
                    rustix::io::read(&descriptor.fd, buffer.initialize_unfilled())
                        .map_err(io::Error::from)
                }
            };
            match read {
                Ok(count) => {
                    buffer.advance(count);
                    return Poll::Ready(Ok(()));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        loop {
            let written = match &self.handle {
                Handle::Ready(handle) => {
                    let mut readiness = ready!(handle.poll_write_ready(context))?;
                    match readiness.try_io(|handle| {
                        rustix::io::write(&handle.get_ref().fd, bytes).map_err(io::Error::from)
                    }) {
                        Ok(result) => result,
                        Err(_) => continue,
                    }
                }
                Handle::File(descriptor) => {
                    rustix::io::write(&descriptor.fd, bytes).map_err(io::Error::from)
                }
            };
            match written {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => return Poll::Ready(result),
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::stdio::Stream;
    use rustix::fs::{OFlags, fcntl_getfl};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn actual_socket_read_cancels_and_restores_shared_status_flags() {
        let (reader, writer) = std::os::unix::net::UnixStream::pair().unwrap();
        let original = fcntl_getfl(&reader).unwrap();
        let mut stream = Stream::new(&reader).unwrap();
        assert!(fcntl_getfl(&reader).unwrap().contains(OFlags::NONBLOCK));
        let mut byte = [0];
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), stream.read(&mut byte))
                .await
                .is_err()
        );
        rustix::io::write(&writer, b"A").unwrap();
        assert_eq!(stream.read(&mut byte).await.unwrap(), 1);
        assert_eq!(byte, *b"A");
        drop(writer);
        assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
        drop(stream);
        assert_eq!(fcntl_getfl(&reader).unwrap(), original);
    }

    #[tokio::test]
    async fn regular_file_input_and_output_work_without_reactor_registration() {
        let mut file = tempfile::tempfile().unwrap();
        let original = fcntl_getfl(&file).unwrap();
        let mut stream = Stream::new(&file).unwrap();
        stream.write_all(b"protocol\n").await.unwrap();
        drop(stream);
        std::io::Seek::rewind(&mut file).unwrap();
        let mut stream = Stream::new(&file).unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).await.unwrap();
        assert_eq!(text, "protocol\n");
        drop(stream);
        assert_eq!(fcntl_getfl(&file).unwrap(), original);
    }
}
