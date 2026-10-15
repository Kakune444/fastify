//! Spotify library protocol. No retries for writes: an uncertain append must not run twice.
use librespot_core::{SpotifyUri, session::Session};
use librespot_protocol::collection2v2::{CollectionItem, PageRequest, PageResponse, WriteRequest};
use protobuf::Message;
use reqwest::{Client, Method};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const COLLECTION_TYPE: &str = "application/vnd.collection-v2.spotify.proto";
pub const PAGE_SIZE: i32 = 40;

#[derive(Debug)]
pub enum LibraryError {
    Input,
    Auth,
    Permission,
    Missing,
    Conflict,
    RateLimited(u64),
    Network,
    Uncertain,
    Protocol,
}
pub type Result<T> = std::result::Result<T, LibraryError>;

pub struct LibraryApi<'a> {
    session: &'a Session,
    client: &'a Client,
}

impl<'a> LibraryApi<'a> {
    pub fn new(session: &'a Session, client: &'a Client) -> Self {
        Self { session, client }
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        content_type: &str,
        body: Vec<u8>,
        mutation: bool,
    ) -> Result<Vec<u8>> {
        let prepare = async {
            let base = self
                .session
                .spclient()
                .base_url()
                .await
                .map_err(|_| LibraryError::Network)?;
            let token = self
                .session
                .login5()
                .auth_token()
                .await
                .map_err(|_| LibraryError::Auth)?;
            let client = self
                .session
                .spclient()
                .client_token()
                .await
                .map_err(|_| LibraryError::Network)?;
            Ok::<_, LibraryError>((base, token.access_token, client))
        };
        let (base, token, client) = tokio::time::timeout(Duration::from_secs(20), prepare)
            .await
            .map_err(|_| LibraryError::Network)??;
        let response = self
            .client
            .request(method, format!("{base}{path}"))
            .bearer_auth(token)
            .header("client-token", client)
            .header("content-type", content_type)
            .header("accept", content_type)
            .body(body)
            .send()
            .await
            .map_err(|_| {
                if mutation {
                    LibraryError::Uncertain
                } else {
                    LibraryError::Network
                }
            })?;
        match response.status().as_u16() {
            200..=299 => {}
            401 => return Err(LibraryError::Auth),
            403 => return Err(LibraryError::Permission),
            404 => return Err(LibraryError::Missing),
            409 => return Err(LibraryError::Conflict),
            429 => {
                return Err(LibraryError::RateLimited(
                    response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(60)
                        .clamp(1, 3600),
                ));
            }
            _ => {
                return Err(if mutation {
                    LibraryError::Uncertain
                } else {
                    LibraryError::Network
                });
            }
        }
        if response
            .content_length()
            .is_some_and(|size| size > 2_000_000)
        {
            return Err(LibraryError::Protocol);
        }
        let bytes = response.bytes().await.map_err(|_| {
            if mutation {
                LibraryError::Uncertain
            } else {
                LibraryError::Network
            }
        })?;
        if bytes.len() > 2_000_000 {
            return Err(LibraryError::Protocol);
        }
        Ok(bytes.to_vec())
    }

    fn items_request(&self, uris: &[String], saved: Option<bool>) -> Result<WriteRequest> {
        if uris.is_empty() || uris.len() > 50 {
            return Err(LibraryError::Input);
        }
        let mut request = WriteRequest::new();
        request.username = self.session.username();
        request.set = "collection".into();
        for uri in uris {
            let mut item = CollectionItem::new();
            item.uri = track_uri(uri)?;
            if let Some(saved) = saved {
                item.is_removed = !saved;
                if saved {
                    item.added_at = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| LibraryError::Input)?
                        .as_secs()
                        .min(i32::MAX as u64) as i32;
                }
            }
            request.items.push(item);
        }
        Ok(request)
    }

    pub async fn contains(&self, uris: &[String]) -> Result<Vec<bool>> {
        // ContainsRequest shares fields 1–3 with WriteRequest; no write-only fields are set.
        let body = self
            .items_request(uris, None)?
            .write_to_bytes()
            .map_err(|_| LibraryError::Protocol)?;
        let bytes = self
            .request(
                Method::POST,
                "/collection/v2/contains",
                COLLECTION_TYPE,
                body,
                false,
            )
            .await?;
        decode_contains(&bytes, uris.len())
    }

    pub async fn set_saved(&self, uri: &str, saved: bool) -> Result<()> {
        let uris = vec![track_uri(uri)?];
        let body = self
            .items_request(&uris, Some(saved))?
            .write_to_bytes()
            .map_err(|_| LibraryError::Protocol)?;
        match self
            .request(
                Method::POST,
                "/collection/v2/write",
                COLLECTION_TYPE,
                body,
                true,
            )
            .await
        {
            Ok(_) | Err(LibraryError::Uncertain) => {}
            Err(error) => return Err(error),
        }
        if self.contains(&uris).await?.first().copied() == Some(saved) {
            Ok(())
        } else {
            Err(LibraryError::Uncertain)
        }
    }

    pub async fn liked_page(&self, token: &str) -> Result<(Vec<String>, Option<String>)> {
        if token.len() > 4096 {
            return Err(LibraryError::Input);
        }
        let mut request = PageRequest::new();
        request.username = self.session.username();
        request.set = "collection".into();
        request.pagination_token = token.into();
        request.limit = PAGE_SIZE;
        let bytes = self
            .request(
                Method::POST,
                "/collection/v2/paging",
                COLLECTION_TYPE,
                request
                    .write_to_bytes()
                    .map_err(|_| LibraryError::Protocol)?,
                false,
            )
            .await?;
        let page = PageResponse::parse_from_bytes(&bytes).map_err(|_| LibraryError::Protocol)?;
        let uris = page
            .items
            .into_iter()
            .filter(|item| !item.is_removed)
            .filter_map(|item| track_uri(&item.uri).ok())
            .collect();
        let next = if page.next_page_token.is_empty() {
            None
        } else {
            Some(page.next_page_token)
        };
        Ok((uris, next))
    }
}

pub fn track_uri(value: &str) -> Result<String> {
    let uri = SpotifyUri::from_uri(value).map_err(|_| LibraryError::Input)?;
    if !matches!(uri, SpotifyUri::Track { .. }) {
        return Err(LibraryError::Input);
    }
    uri.to_uri().map_err(|_| LibraryError::Input)
}

fn varint(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*cursor).ok_or(LibraryError::Protocol)?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err(LibraryError::Protocol);
        }
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(LibraryError::Protocol)
}

fn decode_contains(bytes: &[u8], expected: usize) -> Result<Vec<bool>> {
    let mut cursor = 0;
    let mut found = Vec::new();
    while cursor < bytes.len() {
        let tag = varint(bytes, &mut cursor)?;
        match (tag >> 3, tag & 7) {
            (1, 0) => found.push(varint(bytes, &mut cursor)? != 0),
            (field, 2) if field > 0 => {
                let length = usize::try_from(varint(bytes, &mut cursor)?)
                    .map_err(|_| LibraryError::Protocol)?;
                let end = cursor
                    .checked_add(length)
                    .filter(|end| *end <= bytes.len())
                    .ok_or(LibraryError::Protocol)?;
                if field == 1 {
                    while cursor < end {
                        found.push(varint(&bytes[..end], &mut cursor)? != 0);
                    }
                } else {
                    cursor = end;
                }
            }
            (field, 0) if field > 0 => {
                varint(bytes, &mut cursor)?;
            }
            (field, wire @ (1 | 5)) if field > 0 => {
                cursor = cursor
                    .checked_add(if wire == 1 { 8 } else { 4 })
                    .filter(|end| *end <= bytes.len())
                    .ok_or(LibraryError::Protocol)?;
            }
            _ => return Err(LibraryError::Protocol),
        }
        if found.len() > expected {
            return Err(LibraryError::Protocol);
        }
    }
    if found.len() != expected {
        return Err(LibraryError::Protocol);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contains_accepts_packed_and_unpacked_fields_and_future_metadata() {
        assert_eq!(
            decode_contains(&[10, 3, 1, 0, 1, 18, 2, 8, 9], 3).unwrap(),
            [true, false, true]
        );
        assert_eq!(decode_contains(&[8, 1, 8, 0], 2).unwrap(), [true, false]);
    }
    #[test]
    fn contains_rejects_truncated_or_misaligned_answers() {
        for data in [&[10, 4, 1, 0][..], &[10, 1, 128][..], &[8, 1][..], &[0][..]] {
            assert!(decode_contains(data, 2).is_err());
        }
    }
    #[test]
    fn mutation_targets_must_be_canonical_tracks() {
        assert_eq!(
            track_uri("spotify:track:2cGxRwrMyEAp8dEbuZaVv6").unwrap(),
            "spotify:track:2cGxRwrMyEAp8dEbuZaVv6"
        );
        assert!(track_uri("spotify:album:2cGxRwrMyEAp8dEbuZaVv6").is_err());
        assert!(track_uri("https://example.com").is_err());
    }
}
