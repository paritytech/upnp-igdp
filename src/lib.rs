// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

#![forbid(unsafe_code)]

mod error;
mod util;
mod xml;

use crate::{error::{Error, Result}, util::{SSDP_SEARCH_REQUEST, SERVICE_TYPE}};
use futures::{future::{self, Either, Loop}, prelude::*};
use log::{debug, trace};
use roxmltree::Document;
use std::{fmt, net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs}, str, time::{Duration, Instant}};
use tokio_timer::Delay;
use tokio_udp::UdpSocket;
use unicase::Ascii;
use url::Url;

/// Try to get our external IP address form a UPnP WANIPConnection.
pub fn external_ip<A>(addrs: A) -> impl Future<Item=Option<IpAddr>, Error=Error>
where
    A: ToSocketAddrs
{
    future::result(Igdp::bind(addrs))
        .and_then(Igdp::discover)
        .and_then(Igdp::control)
        .and_then(Igdp::external_ip)
        .map(|(_, addr)| addr)
}

/// Try to create a port mapping for any external host to the given port.
pub fn port_mapping<A>(addrs: A, p: Protocol, port: u16, dur: Duration, descr: &'static str)
    -> impl Future<Item=Option<u16>, Error=Error>
where
    A: ToSocketAddrs
{
    future::result(Igdp::bind(addrs))
        .and_then(Igdp::discover)
        .and_then(Igdp::control)
        .and_then(move |igdp| {
            igdp.add_port_mapping(p, port, dur, descr)
        })
        .map(|(_, port)| port)
}

/// The protocol for which a port mapping should be created.
#[derive(Clone, Copy, Debug)]
pub enum Protocol { Tcp, Udp }

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Protocol::Tcp => f.write_str("TCP"),
            Protocol::Udp => f.write_str("UDP")
        }
    }
}

/// An instance of the IGD protocol.
#[derive(Debug)]
pub struct Igdp<T> {
    socket: UdpSocket,
    local: IpAddr,
    buffer: Vec<u8>,
    state: T
}

/// `Igdp` state after discovery was successful.
#[derive(Debug)]
pub struct Discovery {
    url: Url,
    addr: SocketAddr
}

/// `Igdp` state after a control URL has been discovered.
#[derive(Debug)]
pub struct Control {
    url: Url,
    addr: SocketAddr
}

impl Igdp<()> {
    /// Create a new Igdp instance, binding the UDP port to the address provided.
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        for a in addr.to_socket_addrs()? {
            if let Ok(socket) = UdpSocket::bind(&a) {
                let local = socket.local_addr()?;
                trace!("new igdp instance bound to {}", local);
                return Ok(Igdp {
                    socket,
                    local: local.ip(),
                    buffer: vec![0; 65527],
                    state: ()
                })
            }
        }
        Err(Error::Bind)
    }

    /// Send SSDP M-SEARCH request to find a UPnP `WANIPConnection`.
    pub fn discover(self) -> impl Future<Item=Igdp<Discovery>, Error=Error> {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(239, 255, 255, 250)), 1900);
        let buff = self.buffer;
        let sock = self.socket;
        let local = self.local;

        // Send M-SEARCH request up to three times and wait 1 sec for response.
        // Since we use UDP, frames may get lost, so retrying seems advisable.
        future::loop_fn((1, sock, buff), move |(i, sock, buff)| {
            if i > 3 {
                return Either::A(future::err(Error::Timeout))
            }
            Either::B(sock.send_dgram(SSDP_SEARCH_REQUEST, &addr).from_err()
                .and_then(move |(sock, _)| {
                    trace!("sent m-search request to {}", addr);
                    sock.recv_dgram(buff)
                        .select2(Delay::new(Instant::now() + Duration::from_secs(1)))
                        .map_err(|error| {
                            match error {
                                Either::A((e, _)) => e.into(),
                                Either::B((_, _)) => Error::Timer
                            }
                        })
                        .and_then(move |result| {
                            match result {
                                Either::A((result, _)) => {
                                    Ok(Loop::Break(result))
                                }
                                Either::B((_, recv)) => {
                                    let parts = recv.into_parts();
                                    Ok(Loop::Continue((i + 1, parts.socket, parts.buffer)))
                                }
                            }
                        })
                }))
        })
        .and_then(move |(sock, buf, n, addr)| {
            trace!("received m-search response from {}", addr);
            let url;
            {
                let mut headers = [httparse::EMPTY_HEADER; 16];
                let mut response = httparse::Response::new(&mut headers);
                let mut location = None;
                response.parse(&buf[.. n])?; // TODO: handle partial
                if Some(200) != response.code {
                    debug!("m-search response code = {:?}", response.code);
                    return Err(Error::StatusCode(response.code))
                }
                for h in response.headers {
                    if Ascii::new(h.name) == "LOCATION" {
                        location = Some(h.value);
                        break
                    }
                }
                if let Some(u) = location
                    .and_then(|loc| str::from_utf8(loc).ok())
                    .and_then(|loc| Url::parse(loc).ok())
                {
                    url = u
                } else {
                    return Err(Error::Location)
                }
            }
            trace!("discovered location: {}", url);
            let addr = util::url2sock(&url)?;
            let disco = Discovery { url, addr };
            Ok(Igdp {
                socket: sock,
                buffer: buf,
                local,
                state: disco
            })
        })
    }
}

impl Igdp<Discovery> {
    /// After we have found an WANIPConnection endpoint, try you figure out
    /// its control URL.
    pub fn control(self) -> impl Future<Item=Igdp<Control>, Error=Error> {
        let req = util::format_get_req(&self.state.addr, self.state.url.path());
        trace!("connecting to {}", self.state.addr);
        util::fetch(self.state.addr, req)
            .and_then(move |bytes| {
                let url = extract_control_url(self.state.url, &bytes[..])?;
                trace!("extracted control url {}", url);
                Ok(Igdp {
                    socket: self.socket,
                    buffer: self.buffer,
                    local: self.local,
                    state: Control { url, addr: self.state.addr }
                })
            })
    }
}

impl Igdp<Control> {
    /// Get our external IP address.
    pub fn external_ip(self) -> impl Future<Item=(Self, Option<IpAddr>), Error=Error> {
        let req = util::format_external_ip(&self.state.addr, self.state.url.path());
        trace!("connecting to {}", self.state.addr);
        util::fetch(self.state.addr, req)
            .and_then(move |bytes| {
                let ext_ip = extract_external_ip(&bytes[..])?;
                trace!("external IP address: {:?}", ext_ip);
                let igdp = Igdp {
                    socket: self.socket,
                    buffer: self.buffer,
                    local: self.local,
                    state: self.state
                };
                Ok((igdp, ext_ip))
            })
    }

    /// Try to create a port mapping, allowing incoming traffic to reach us at the given port.
    pub fn add_port_mapping(self, proto: Protocol, port: u16, dura: Duration, description: &str)
        -> impl Future<Item=(Self, Option<u16>), Error=Error>
    {
        let pmap = util::PortMapping {
            protocol: proto,
            address: self.local,
            port,
            duration: dura,
            description
        };
        let req = util::format_add_any_port_mapping(&self.state.addr, self.state.url.path(), &pmap);
        trace!("connecting to {}", self.state.addr);
        util::fetch(self.state.addr, req)
            .and_then(move |bytes| {
                let port = extract_port_mapping(&bytes[..])?;
                trace!("external port: {:?}", port);
                let igdp = Igdp {
                    socket: self.socket,
                    buffer: self.buffer,
                    local: self.local,
                    state: self.state
                };
                Ok((igdp, port))
            })
    }
}

fn extract_control_url(mut base: Url, description: &[u8]) -> Result<Url> {
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut response = httparse::Response::new(&mut headers);
    match response.parse(description)? {
        httparse::Status::Complete(n) => {
            if Some(200) != response.code {
                return Err(Error::StatusCode(response.code))
            }
            let body_string = str::from_utf8(&description[n ..])?;
            let document = Document::parse(body_string)?;
            for node in document.descendants().filter(|n| n.has_tag_name("service")) {
                let cursor = xml::Cursor::new(node);
                let service = cursor.get("serviceType");
                if Ascii::new(SERVICE_TYPE) != service.text().unwrap_or("") {
                    continue
                }
                let ctrl_url = cursor.get("controlURL");
                if let Some(url) = ctrl_url.text() {
                    base.set_path(url);
                    return Ok(base)
                }
            }
            Err(Error::ControlUrl)
        }
        httparse::Status::Partial => {
            unimplemented!() // TODO
        }
    }
}

fn extract_external_ip(bytes: &[u8]) -> Result<Option<IpAddr>> {
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut response = httparse::Response::new(&mut headers);
    match response.parse(bytes)? {
        httparse::Status::Complete(n) => {
            if Some(200) != response.code {
                return Err(Error::StatusCode(response.code))
            }
            let body_string = str::from_utf8(&bytes[n ..])?;
            let document = Document::parse(body_string)?;
            let cursor = xml::Cursor::new(document.root());
            let ext_ip = cursor
                .get("Envelope")
                .get("Body")
                .get("GetExternalIPAddressResponse")
                .get("NewExternalIPAddress");
            Ok(ext_ip.text().and_then(|s| s.parse().ok()))
        }
        httparse::Status::Partial => {
            unimplemented!() // TODO
        }
    }
}

fn extract_port_mapping(bytes: &[u8]) -> Result<Option<u16>> {
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut response = httparse::Response::new(&mut headers);
    match response.parse(bytes)? {
        httparse::Status::Complete(n) => {
            if Some(200) != response.code {
                return Err(Error::StatusCode(response.code))
            }
            let body_string = str::from_utf8(&bytes[n ..])?;
            let document = Document::parse(body_string)?;
            let cursor = xml::Cursor::new(document.root());
            let port = cursor
                .get("Envelope")
                .get("Body")
                .get("AddAnyPortMapping")
                .get("NewReservedPort");
            Ok(port.text().and_then(|s| s.parse().ok()))
        }
        httparse::Status::Partial => {
            unimplemented!() // TODO
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate env_logger;
    extern crate tokio;
    use super::*;

    #[test]
    fn test_external_ip() {
        let _ = env_logger::try_init();
        let f = external_ip("0.0.0.0:0")
            .map(|_ip| ())
            .map_err(|e| panic!("external_ip failed with error: {}", e));
        tokio::run(f)
    }

    #[test]
    fn test_port_mapping() {
        let _ = env_logger::try_init();
        let f = port_mapping("0.0.0.0:0", Protocol::Tcp, 33445, Duration::from_secs(10), "test")
            .map(|_port| ())
            .map_err(|e| panic!("port_mapping failed with error: {}", e));
        tokio::run(f)
    }
}

