use smoltcp::socket::{UdpPacketMetadata, UdpSocket, UdpSocketBuffer};
use smoltcp::iface::{SocketHandle};
use smoltcp::wire::IpEndpoint;
use std::str;
use syscall;
use syscall::{Error as SyscallError, Result as SyscallResult};

use super::socket::{DupResult, SchemeFile, SchemeSocket, SocketFile, SocketScheme};
use super::{parse_endpoint, Smolnetd, SmolnetInterface};
use device::NetworkDevice;
use port_set::PortSet;

pub type UdpScheme = SocketScheme<UdpSocket<'static>>;

impl<'a> SchemeSocket for UdpSocket<'a> {
    type SchemeDataT = PortSet;
    type DataT = IpEndpoint;
    type SettingT = ();

    fn new_scheme_data() -> Self::SchemeDataT {
        PortSet::new(49_152u16, 65_535u16).expect("Wrong UDP port numbers")
    }

    fn can_send(&self) -> bool {
        self.can_send()
    }

    fn can_recv(&self) -> bool {
        self.can_recv()
    }

    fn may_recv(&self) -> bool {
        true
    }

    fn hop_limit(&self) -> u8 {
        self.hop_limit().unwrap_or(64)
    }

    fn set_hop_limit(&mut self, hop_limit: u8) {
        self.set_hop_limit(Some(hop_limit));
    }

    fn get_setting(
        _file: &SocketFile<Self::DataT>,
        _setting: Self::SettingT,
        _buf: &mut [u8],
    ) -> SyscallResult<usize> {
        Ok(0)
    }

    fn set_setting(
        _file: &mut SocketFile<Self::DataT>,
        _setting: Self::SettingT,
        _buf: &[u8],
    ) -> SyscallResult<usize> {
        Ok(0)
    }

    fn new_socket(
        iface: &mut SmolnetInterface,
        path: &str,
        uid: u32,
        port_set: &mut Self::SchemeDataT,
    ) -> SyscallResult<(SocketHandle, Self::DataT)> {
        trace!("UDP open {}", path);
        let mut parts = path.split('/');
        let remote_endpoint = parse_endpoint(parts.next().unwrap_or(""));
        let mut local_endpoint = parse_endpoint(parts.next().unwrap_or(""));

        if local_endpoint.port > 0 && local_endpoint.port <= 1024 && uid != 0 {
            return Err(SyscallError::new(syscall::EACCES));
        }

        let rx_buffer = UdpSocketBuffer::new(
            vec![UdpPacketMetadata::EMPTY; Smolnetd::SOCKET_BUFFER_SIZE],
            vec![0; NetworkDevice::MTU * Smolnetd::SOCKET_BUFFER_SIZE],
        );
        let tx_buffer = UdpSocketBuffer::new(
            vec![UdpPacketMetadata::EMPTY; Smolnetd::SOCKET_BUFFER_SIZE],
            vec![0; NetworkDevice::MTU * Smolnetd::SOCKET_BUFFER_SIZE],
        );
        let udp_socket = UdpSocket::new(rx_buffer, tx_buffer);

        if local_endpoint.port == 0 {
            local_endpoint.port = port_set
                .get_port()
                .ok_or_else(|| SyscallError::new(syscall::EINVAL))?;
        } else if !port_set.claim_port(local_endpoint.port) {
            return Err(SyscallError::new(syscall::EADDRINUSE));
        }

        let socket_handle = iface.add_socket(udp_socket);
        trace!("UDP add socket {}", socket_handle);

        let udp_socket = iface.get_socket::<UdpSocket>(socket_handle);
        udp_socket
            .bind(local_endpoint)
            .expect("Can't bind udp socket to local endpoint");
        trace!("UDP bind socket {}", socket_handle);

        Ok((socket_handle, remote_endpoint))
    }

    fn close_file(
        &self,
        file: &SchemeFile<Self>,
        port_set: &mut Self::SchemeDataT,
    ) -> SyscallResult<()> {
        if let SchemeFile::Socket(_) = *file {
            port_set.release_port(self.endpoint().port);
        }
        Ok(())
    }

    fn write_buf(
        &mut self,
        file: &mut SocketFile<Self::DataT>,
        buf: &[u8],
    ) -> SyscallResult<Option<usize>> {
        if !file.data.is_specified() {
            return Err(SyscallError::new(syscall::EADDRNOTAVAIL));
        }
        if self.can_send() {
            self.send_slice(buf, file.data).expect("Can't send slice");
            Ok(Some(buf.len()))
        } else if file.flags & syscall::O_NONBLOCK == syscall::O_NONBLOCK {
            Err(SyscallError::new(syscall::EAGAIN))
        } else {
            Ok(None) // internally scheduled to re-read
        }
    }

    fn read_buf(
        &mut self,
        file: &mut SocketFile<Self::DataT>,
        buf: &mut [u8],
    ) -> SyscallResult<Option<usize>> {
        if self.can_recv() {
            let (length, _) = self.recv_slice(buf).expect("Can't receive slice");
            Ok(Some(length))
        } else if file.flags & syscall::O_NONBLOCK == syscall::O_NONBLOCK {
            Err(SyscallError::new(syscall::EAGAIN))
        } else {
            Ok(None) // internally scheduled to re-read
        }
    }

    fn dup(
        iface: &mut SmolnetInterface,
        file: &mut SchemeFile<Self>,
        path: &str,
        port_set: &mut Self::SchemeDataT,
    ) -> SyscallResult<DupResult<Self>> {
        trace!("duping...");
        let socket_handle = file.socket_handle();
        let file = match path {
            _ => {
                let remote_endpoint = parse_endpoint(path);
                if let SchemeFile::Socket(ref udp_handle) = *file {
                    SchemeFile::Socket(udp_handle.clone_with_data(
                        if remote_endpoint.is_specified() {
                            remote_endpoint
                        } else {
                            udp_handle.data
                        },
                    ))
                } else {
                    SchemeFile::Socket(SocketFile::new_with_data(socket_handle, remote_endpoint))
                }
            }
        };

        let endpoint = {
            let socket = iface.get_socket::<UdpSocket>(socket_handle);
            socket.endpoint()
        };

        if let SchemeFile::Socket(_) = file {
            port_set.acquire_port(endpoint.port);
        }

        Ok(Some((file, None)))
    }

    fn fpath(&self, file: &SchemeFile<Self>, buf: &mut [u8]) -> SyscallResult<usize> {
        if let SchemeFile::Socket(ref socket_file) = *file {
            let path = format!("udp:{}/{}", socket_file.data, self.endpoint());
            let path = path.as_bytes();

            let mut i = 0;
            while i < buf.len() && i < path.len() {
                buf[i] = path[i];
                i += 1;
            }

            Ok(i)
        } else {
            Err(SyscallError::new(syscall::EBADF))
        }
    }
}
