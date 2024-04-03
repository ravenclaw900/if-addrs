// Copyright 2018 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under the MIT license <LICENSE-MIT
// http://opensource.org/licenses/MIT> or the Modified BSD license <LICENSE-BSD
// https://opensource.org/licenses/BSD-3-Clause>, at your option. This file may not be copied,
// modified, or distributed except according to those terms. Please review the Licences for the
// specific language governing permissions and limitations relating to use of the SAFE Network
// Software.

use crate::sockaddr;
use libc::{
    bind, close, freeifaddrs, getifaddrs, ifaddrs, sockaddr_nl, socket, AF_NETLINK, NETLINK_ROUTE, SOCK_RAW
};
use std::net::{IpAddr, UdpSocket};
use std::os::fd::FromRawFd;
use std::time::Duration;
use std::{io, mem};

pub fn do_broadcast(ifaddr: &ifaddrs) -> Option<IpAddr> {
    // On Linux-like systems, `ifa_ifu` is a union of `*ifa_dstaddr` and `*ifa_broadaddr`.
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "l4re",
        target_os = "emscripten",
        target_os = "fuchsia",
        target_os = "hurd",
        target_os = "nacl",
    ))]
    let sockaddr = ifaddr.ifa_ifu;

    // On BSD-like and embedded systems, only `ifa_dstaddr` is available.
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "l4re",
        target_os = "emscripten",
        target_os = "fuchsia",
        target_os = "hurd",
        target_os = "nacl",
    )))]
    let sockaddr = ifaddr.ifa_dstaddr;

    sockaddr::to_ipaddr(sockaddr)
}

pub struct IfAddrs {
    inner: *mut ifaddrs,
}

impl IfAddrs {
    #[allow(unsafe_code, clippy::new_ret_no_self)]
    pub fn new() -> io::Result<Self> {
        let mut ifaddrs = mem::MaybeUninit::uninit();

        unsafe {
            if -1 == getifaddrs(ifaddrs.as_mut_ptr()) {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                inner: ifaddrs.assume_init(),
            })
        }
    }

    pub fn iter(&self) -> IfAddrsIterator {
        IfAddrsIterator { next: self.inner }
    }
}

impl Drop for IfAddrs {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        unsafe {
            freeifaddrs(self.inner);
        }
    }
}

pub struct IfAddrsIterator {
    next: *mut ifaddrs,
}

impl Iterator for IfAddrsIterator {
    type Item = ifaddrs;

    #[allow(unsafe_code)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.next.is_null() {
            return None;
        };

        Some(unsafe {
            let result = *self.next;
            self.next = (*self.next).ifa_next;

            result
        })
    }
}

/// Block until the OS reports that the network interface list has changed, or
/// until an optional timeout. Returns an [`io::ErrorKind::WouldBlock`] error on
/// timeout, or another error if the network notifier could not be set up.
pub fn detect_interface_changes(timeout: Option<Duration>) -> io::Result<()> {
    let socket = unsafe { socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE) };
    if socket < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut sockaddr: sockaddr_nl = unsafe { mem::zeroed() };
    sockaddr.nl_family = AF_NETLINK as u16;
    sockaddr.nl_groups = 1; // RTNLGRP_LINK

    if unsafe {
        bind(
            socket,
            &sockaddr as *const _ as *const libc::sockaddr,
            mem::size_of::<sockaddr_nl>() as libc::socklen_t,
        )
    } < 0
    {
        unsafe { close(socket) };
        return Err(io::Error::last_os_error());
    }

    // lie about the type, since they all use fds and we don't need specifics
    // after we have called bind
    let socket = unsafe { UdpSocket::from_raw_fd(socket) };

    let mut buf = [0u8; 65536];
    socket.set_read_timeout(timeout)?;
    socket.recv(&mut buf)?;

    Ok(())
}
