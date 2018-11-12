// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Licensed under the Apache License, Version 2.0 or MIT license, at your option.
//
// A copy of the Apache License, Version 2.0 is included in the software as
// LICENSE-APACHE and a copy of the MIT license is included in the software
// as LICENSE-MIT. You may also obtain a copy of the Apache License, Version 2.0
// at https://www.apache.org/licenses/LICENSE-2.0 and a copy of the MIT license
// at https://opensource.org/licenses/MIT.

use bytes::Bytes;
use crate::error::{Error, Result};
use futures::prelude::*;
use log::trace;
use std::{net::{IpAddr, SocketAddr}, time::Duration};
use tokio_codec::{FramedRead, FramedWrite, BytesCodec};
use tokio_tcp::TcpStream;
use url::{Host, Url};

pub(crate) const SSDP_SEARCH_REQUEST: &[u8] =
    b"M-SEARCH * HTTP/1.1\r\n\
    Host: 239.255.255.250:1900\r\n\
    MAN: \"ssdp:discover\"\r\n\
    MX: 1\r\n\
    ST: urn:schemas-upnp-org:service:WANIPConnection:2\r\n\
    CPFN.UPNP.ORG: upnp-igdp-crate\r\n\r\n";

pub(crate) const SERVICE_TYPE: &str =
    "urn:schemas-upnp-org:service:WANIPConnection:2";

pub(crate) const GET_EXTERNAL_IP_SOAP_ENV: &str =
    r#"<?xml version="1.0" encoding="utf-8"?>
    <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
        <s:Body>
            <u:GetExternalIPAddress xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:2"/>
        </s:Body>
    </s:Envelope>
    "#;

pub(crate) fn url2sock(url: &Url) -> Result<SocketAddr> {
    match (url.host(), url.port()) {
        (Some(Host::Ipv4(addr)), Some(port)) => Ok(SocketAddr::new(IpAddr::V4(addr), port)),
        (Some(Host::Ipv6(addr)), Some(port)) => Ok(SocketAddr::new(IpAddr::V6(addr), port)),
        _                                    => Err(Error::HostPort)
    }
}

pub(crate) fn fetch(addr: SocketAddr, req: String) -> impl Future<Item=Bytes, Error=Error> {
    TcpStream::connect(&addr)
        .from_err()
        .and_then(move |conn| {
            trace!("sending request to {}", addr);
            let codec = FramedWrite::new(conn, BytesCodec::new());
            codec.send(req.into_bytes().into()).from_err().map(|codec| codec.into_inner())
        })
        .and_then(move |conn| {
            trace!("reading response from {}", addr);
            let codec = FramedRead::new(conn, BytesCodec::new());
            codec.concat2().from_err().map(|b| b.freeze()) // TODO: Timeout
        })
}

pub(crate) fn format_get_req(host: &SocketAddr, path: &str) -> String {
    format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host)
}

pub(crate) fn format_external_ip(host: &SocketAddr, path: &str) -> String {
    format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Length: {}\r\n\
         Content-Type: text/xml\r\n\
         SOAPAction: \"urn:schemas-upnp-org:service:WANIPConnection:2#GetExternalIPAddress\"\r\n\
         Connection: Close\r\n\r\n\
         {}
        ", path, host, GET_EXTERNAL_IP_SOAP_ENV.len(), GET_EXTERNAL_IP_SOAP_ENV)
}

pub(crate) struct PortMapping<'a> {
    pub(crate) protocol: super::Protocol,
    pub(crate) address: IpAddr,
    pub(crate) port: u16,
    pub(crate) description: &'a str,
    pub(crate) duration: Duration
}

pub(crate) fn format_add_any_port_mapping(host: &SocketAddr, path: &str, pm: &PortMapping) -> String {
    let body = format!(r#"<?xml version="1.0" encoding="utf-8"?>
        <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
            <s:Body>
                <u:AddAnyPortMapping xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:2">
                    <u:NewRemoteHost/>
                    <u:NewExternalPort>0</u:NewExternalPort>
                    <u:NewProtocol>{}</u:NewProtocol>
                    <u:NewInternalPort>{}</u:NewInternalPort>
                    <u:NewInternalClient>{}</u:NewInternalClient>
                    <u:NewEnabled>true</u:NewEnabled>
                    <u:NewPortMappingDescription>{}</u:NewPortMappingDescription>
                    <u:NewLeaseDuration>{}</u:NewLeaseDuration>
                </u:AddAnyPortMapping>
            </s:Body>
        </s:Envelope>
        "#, pm.protocol, pm.port, pm.address, pm.description, pm.duration.as_secs());

    format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}\r\n\
         Content-Length: {}\r\n\
         Content-Type: text/xml\r\n\
         SOAPAction: \"urn:schemas-upnp-org:service:WANIPConnection:2#AddAnyPortMapping\"\r\n\
         Connection: Close\r\n\r\n\
         {}
        ", path, host, body.len(), body)
}
