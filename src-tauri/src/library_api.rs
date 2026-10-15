//! Spotify library protocol. No retries for writes: an uncertain append must not run twice.
use librespot_core::{SpotifyUri, session::Session};
mod wire {
    include!(concat!(env!("OUT_DIR"), "/library_proto/mod.rs"));
}
use librespot_protocol::playlist4_external as p;
use protobuf::Message;
use reqwest::{Client, Method};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use wire::collection::{
    CollectionItem, ContainsRequest, ContainsResponse, PageRequest, PageResponse, WriteRequest,
};

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

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PlaylistEdit {
    Details {
        name: String,
        description: String,
    },
    Add {
        uris: Vec<String>,
    },
    Remove {
        position: usize,
        uri: String,
    },
    Move {
        position: usize,
        to: usize,
        uri: String,
    },
}

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
                        .min(i64::MAX as u64) as i64;
                }
            }
            request.items.push(item);
        }
        Ok(request)
    }

    pub async fn contains(&self, uris: &[String]) -> Result<Vec<bool>> {
        let items = self.items_request(uris, None)?;
        let mut request = ContainsRequest::new();
        request.username = items.username;
        request.set = items.set;
        request.items = items.items;
        let body = request
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
        // The collection contains tombstones and other entity types. Skip empty
        // track pages, while bounding the work and preserving the server cursor.
        for page_index in 0..10 {
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
            let page =
                PageResponse::parse_from_bytes(&bytes).map_err(|_| LibraryError::Protocol)?;
            let uris: Vec<String> = page
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
            if !uris.is_empty() || next.is_none() || page_index == 9 {
                return Ok((uris, next));
            }
            let next = next.unwrap();
            if next == request.pagination_token {
                return Err(LibraryError::Protocol);
            }
            request.pagination_token = next;
        }
        Err(LibraryError::Protocol)
    }

    pub async fn playlist_page(
        &self,
        uri: &str,
        offset: usize,
        length: usize,
    ) -> Result<p::SelectedListContent> {
        let id = playlist_id(uri)?;
        if offset > 100_000 || length > 1000 {
            return Err(LibraryError::Input);
        }
        let bytes = self.request(Method::GET, &format!("/playlist/v2/playlist/{id}?decorate=revision,attributes,length,owner,capabilities&from={offset}&length={length}"), "application/x-protobuf", vec![], false).await?;
        p::SelectedListContent::parse_from_bytes(&bytes).map_err(|_| LibraryError::Protocol)
    }

    pub async fn edit_playlist(
        &self,
        uri: &str,
        revision: Option<&str>,
        edit: &PlaylistEdit,
    ) -> Result<()> {
        let position = match edit {
            PlaylistEdit::Remove { position, .. } | PlaylistEdit::Move { position, .. } => {
                *position
            }
            _ => 0,
        };
        let before = self.playlist_page(uri, position, 1).await?;
        let (metadata, items) = playlist_permissions(&before, &self.session.username());
        if !match edit {
            PlaylistEdit::Details { .. } => metadata,
            _ => items,
        } {
            return Err(LibraryError::Permission);
        }
        if let Some(revision) = revision {
            if revision_bytes(revision)? != before.revision() {
                return Err(LibraryError::Conflict);
            }
        } else if !matches!(edit, PlaylistEdit::Add { .. }) {
            return Err(LibraryError::Input);
        }
        let op = playlist_operation(edit, &before)?;
        let body = playlist_changes(before.revision(), op, &self.session.username())?
            .write_to_bytes()
            .map_err(|_| LibraryError::Protocol)?;
        let response = self
            .request(
                Method::POST,
                &format!("/playlist/v2/playlist/{}/changes", playlist_id(uri)?),
                "application/x-protobuf",
                body,
                true,
            )
            .await;
        if let Err(error) = response {
            if !matches!(error, LibraryError::Uncertain) {
                return Err(error);
            }
        }
        // Read back without resending a write, including after a lost HTTP response.
        let (check_offset, check_length) = match edit {
            PlaylistEdit::Add { uris } => (before.length().max(0) as usize, uris.len()),
            PlaylistEdit::Move { to, .. } => (*to, 1),
            _ => (0, 1),
        };
        let after = self
            .playlist_page(uri, check_offset, check_length)
            .await
            .map_err(|_| LibraryError::Uncertain)?;
        let verified = match edit {
            PlaylistEdit::Details { name, description } => after
                .attributes
                .as_ref()
                .is_some_and(|a| a.name() == name.trim() && a.description() == description),
            PlaylistEdit::Add { uris } => {
                after.length() == before.length() + uris.len() as i32
                    && after.contents.as_ref().is_some_and(|c| {
                        c.items.len() == uris.len()
                            && c.items
                                .iter()
                                .zip(uris)
                                .all(|(item, uri)| item.uri() == uri)
                    })
            }
            PlaylistEdit::Remove { .. } => {
                after.length() == before.length() - 1 && after.revision() != before.revision()
            }
            PlaylistEdit::Move { uri, .. } => {
                after.length() == before.length()
                    && after
                        .contents
                        .as_ref()
                        .and_then(|c| c.items.first())
                        .is_some_and(|item| item.uri() == uri)
            }
        };
        if verified {
            Ok(())
        } else {
            Err(LibraryError::Uncertain)
        }
    }

    pub async fn create_playlist(&self, name: &str, description: &str) -> Result<String> {
        validate_details(name, description)?;
        let mut values = p::ListAttributes::new();
        values.set_name(name.trim().into());
        values.set_description(description.into());
        let mut partial = p::ListAttributesPartialState::new();
        partial.values = Some(values).into();
        let mut update = p::UpdateListAttributes::new();
        update.new_attributes = Some(partial).into();
        let mut op = p::Op::new();
        op.set_kind(p::op::Kind::UPDATE_LIST_ATTRIBUTES);
        op.update_list_attributes = Some(update).into();
        let mut delta = p::Delta::new();
        delta.ops.push(op);
        let bytes = self
            .request(
                Method::POST,
                "/playlist/v2/playlist",
                "application/x-protobuf",
                delta.write_to_bytes().map_err(|_| LibraryError::Protocol)?,
                true,
            )
            .await?;
        let created =
            p::CreateListReply::parse_from_bytes(&bytes).map_err(|_| LibraryError::Uncertain)?;
        let uri = format!("spotify:playlist:{}", playlist_id(created.uri())?);
        // New playlists are private. Neither profile publication nor collaborator permissions are inherited.
        self.make_private(&uri)
            .await
            .map_err(|_| LibraryError::Uncertain)?;
        self.link_playlist(&uri)
            .await
            .map_err(|_| LibraryError::Uncertain)?;
        Ok(uri)
    }

    async fn make_private(&self, uri: &str) -> Result<()> {
        let path = format!(
            "/playlist-permission/v1/playlist/{}/permission/base",
            playlist_id(uri)?
        );
        let response = self
            .request(Method::GET, &path, "application/json", vec![], false)
            .await?;
        let mut permission: serde_json::Value =
            serde_json::from_slice(&response).map_err(|_| LibraryError::Protocol)?;
        permission["permissionLevel"] = serde_json::json!("BLOCKED");
        self.request(
            Method::POST,
            &path,
            "application/json",
            serde_json::to_vec(&permission).map_err(|_| LibraryError::Protocol)?,
            true,
        )
        .await?;
        let response = self
            .request(Method::GET, &path, "application/json", vec![], false)
            .await?;
        let confirmed: serde_json::Value =
            serde_json::from_slice(&response).map_err(|_| LibraryError::Protocol)?;
        if confirmed["permissionLevel"] == "BLOCKED" || confirmed["permissionLevel"] == 1 {
            Ok(())
        } else {
            Err(LibraryError::Uncertain)
        }
    }

    fn root_path(&self) -> Result<String> {
        let mut url = reqwest::Url::parse("https://spotify.invalid/playlist/v2/user/")
            .map_err(|_| LibraryError::Input)?;
        url.path_segments_mut()
            .map_err(|_| LibraryError::Input)?
            .pop_if_empty()
            .push(&self.session.username())
            .push("rootlist");
        Ok(url.path().into())
    }

    pub async fn link_playlist(&self, uri: &str) -> Result<()> {
        let uri = format!("spotify:playlist:{}", playlist_id(uri)?);
        let path = self.root_path()?;
        let bytes = self
            .request(
                Method::GET,
                &format!("{path}?decorate=revision&from=0&length=1000"),
                "application/x-protobuf",
                vec![],
                false,
            )
            .await?;
        let root =
            p::SelectedListContent::parse_from_bytes(&bytes).map_err(|_| LibraryError::Protocol)?;
        if root.contents.as_ref().is_some_and(|c| {
            c.items
                .iter()
                .any(|item| playlist_id(item.uri()).ok() == playlist_id(&uri).ok())
        }) {
            return Ok(());
        }
        let mut item = p::Item::new();
        item.set_uri(uri);
        let mut attributes = p::ItemAttributes::new();
        attributes.set_public(false);
        item.attributes = Some(attributes).into();
        let mut add = p::Add::new();
        add.set_add_first(true);
        add.items.push(item);
        let mut op = p::Op::new();
        op.set_kind(p::op::Kind::ADD);
        op.add = Some(add).into();
        let changes = playlist_changes(root.revision(), op, &self.session.username())?;
        self.request(
            Method::POST,
            &format!("{path}/changes"),
            "application/x-protobuf",
            changes
                .write_to_bytes()
                .map_err(|_| LibraryError::Protocol)?,
            true,
        )
        .await?;
        Ok(())
    }

    /// Used by the developer smoke test to remove only the playlist it just created.
    pub async fn unlink_playlist(&self, uri: &str) -> Result<()> {
        let id = playlist_id(uri)?;
        let path = self.root_path()?;
        let bytes = self
            .request(
                Method::GET,
                &format!("{path}?decorate=revision&from=0&length=1000"),
                "application/x-protobuf",
                vec![],
                false,
            )
            .await?;
        let root =
            p::SelectedListContent::parse_from_bytes(&bytes).map_err(|_| LibraryError::Protocol)?;
        let Some(position) = root.contents.as_ref().and_then(|c| {
            c.items
                .iter()
                .position(|item| playlist_id(item.uri()).ok().as_ref() == Some(&id))
        }) else {
            return Err(LibraryError::Missing);
        };
        let mut rem = p::Rem::new();
        rem.set_from_index(position as i32);
        rem.set_length(1);
        rem.set_items_as_key(false);
        let mut op = p::Op::new();
        op.set_kind(p::op::Kind::REM);
        op.rem = Some(rem).into();
        let changes = playlist_changes(root.revision(), op, &self.session.username())?;
        self.request(
            Method::POST,
            &format!("{path}/changes"),
            "application/x-protobuf",
            changes
                .write_to_bytes()
                .map_err(|_| LibraryError::Protocol)?,
            true,
        )
        .await?;
        Ok(())
    }
}

pub fn playlist_id(value: &str) -> Result<String> {
    let uri = SpotifyUri::from_uri(value).map_err(|_| LibraryError::Input)?;
    if !matches!(uri, SpotifyUri::Playlist { .. }) {
        return Err(LibraryError::Input);
    }
    uri.to_id().map_err(|_| LibraryError::Input)
}
pub fn revision_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn revision_bytes(value: &str) -> Result<Vec<u8>> {
    if value.is_empty() || value.len() > 256 || value.len() % 2 != 0 || !value.is_ascii() {
        return Err(LibraryError::Input);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|chunk| {
            u8::from_str_radix(
                std::str::from_utf8(chunk).map_err(|_| LibraryError::Input)?,
                16,
            )
            .map_err(|_| LibraryError::Input)
        })
        .collect()
}
pub fn playlist_permissions(snapshot: &p::SelectedListContent, username: &str) -> (bool, bool) {
    let owner = !username.is_empty() && snapshot.owner_username() == username;
    snapshot
        .capabilities
        .as_ref()
        .map(|c| {
            (
                c.can_edit_metadata.unwrap_or(owner),
                c.can_edit_items.unwrap_or(owner),
            )
        })
        .unwrap_or((owner, owner))
}
fn validate_details(name: &str, description: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.chars().count() > 100
        || description.chars().count() > 300
        || name.contains('\0')
        || description.contains('\0')
    {
        Err(LibraryError::Input)
    } else {
        Ok(())
    }
}
fn playlist_changes(revision: &[u8], op: p::Op, username: &str) -> Result<p::ListChanges> {
    if revision.is_empty() {
        return Err(LibraryError::Protocol);
    }
    let mut info = p::ChangeInfo::new();
    info.set_user(username.into());
    info.set_timestamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| LibraryError::Input)?
            .as_millis()
            .min(i64::MAX as u128) as i64,
    );
    let mut delta = p::Delta::new();
    delta.ops.push(op);
    delta.info = Some(info).into();
    let mut changes = p::ListChanges::new();
    changes.set_base_revision(revision.to_vec());
    changes.deltas.push(delta);
    Ok(changes)
}
fn playlist_operation(edit: &PlaylistEdit, before: &p::SelectedListContent) -> Result<p::Op> {
    let mut op = p::Op::new();
    match edit {
        PlaylistEdit::Details { name, description } => {
            validate_details(name, description)?;
            let mut values = p::ListAttributes::new();
            values.set_name(name.trim().into());
            values.set_description(description.clone());
            let mut partial = p::ListAttributesPartialState::new();
            partial.values = Some(values).into();
            let mut update = p::UpdateListAttributes::new();
            update.new_attributes = Some(partial).into();
            op.set_kind(p::op::Kind::UPDATE_LIST_ATTRIBUTES);
            op.update_list_attributes = Some(update).into();
        }
        PlaylistEdit::Add { uris } => {
            if uris.is_empty() || uris.len() > 50 {
                return Err(LibraryError::Input);
            }
            let mut add = p::Add::new();
            add.set_add_last(true);
            for uri in uris {
                let mut item = p::Item::new();
                item.set_uri(track_uri(uri)?);
                add.items.push(item);
            }
            op.set_kind(p::op::Kind::ADD);
            op.add = Some(add).into();
        }
        PlaylistEdit::Remove { position, uri } | PlaylistEdit::Move { position, uri, .. } => {
            let expected = track_uri(uri)?;
            if *position >= before.length().max(0) as usize
                || before
                    .contents
                    .as_ref()
                    .and_then(|c| c.items.first())
                    .is_none_or(|item| item.uri() != expected)
            {
                return Err(LibraryError::Conflict);
            }
            if let PlaylistEdit::Move { to, .. } = edit {
                if *to >= before.length().max(0) as usize || *to == *position {
                    return Err(LibraryError::Input);
                }
                let mut mov = p::Mov::new();
                mov.set_from_index(*position as i32);
                mov.set_length(1);
                mov.set_to_index(if to > position { *to + 1 } else { *to } as i32);
                op.set_kind(p::op::Kind::MOV);
                op.mov = Some(mov).into();
            } else {
                let mut rem = p::Rem::new();
                rem.set_from_index(*position as i32);
                rem.set_length(1);
                rem.set_items_as_key(false);
                op.set_kind(p::op::Kind::REM);
                op.rem = Some(rem).into();
            }
        }
    }
    Ok(op)
}

pub fn track_uri(value: &str) -> Result<String> {
    let uri = SpotifyUri::from_uri(value).map_err(|_| LibraryError::Input)?;
    if !matches!(uri, SpotifyUri::Track { .. }) {
        return Err(LibraryError::Input);
    }
    uri.to_uri().map_err(|_| LibraryError::Input)
}

fn decode_contains(bytes: &[u8], expected: usize) -> Result<Vec<bool>> {
    // Validate every field's framing before decoding packed booleans. The
    // generated packed reader may accept EOF before its declared byte length.
    let mut framing = protobuf::CodedInputStream::from_bytes(bytes);
    let mut unknown = protobuf::UnknownFields::new();
    while let Some(tag) = framing
        .read_raw_tag_or_eof()
        .map_err(|_| LibraryError::Protocol)?
    {
        protobuf::rt::read_unknown_or_skip_group(tag, &mut framing, &mut unknown)
            .map_err(|_| LibraryError::Protocol)?;
    }
    let found = ContainsResponse::parse_from_bytes(bytes)
        .map_err(|_| LibraryError::Protocol)?
        .found;
    if found.len() != expected {
        return Err(LibraryError::Protocol);
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn playlist_fixture() -> p::SelectedListContent {
        let mut before = p::SelectedListContent::new();
        before.set_length(5);
        before.set_revision(vec![1, 2]);
        before.set_owner_username("owner".into());
        let mut contents = p::ListItems::new();
        contents.set_pos(2);
        contents.set_truncated(true);
        let mut item = p::Item::new();
        item.set_uri("spotify:track:2cGxRwrMyEAp8dEbuZaVv6".into());
        contents.items.push(item);
        before.contents = Some(contents).into();
        before
    }
    #[test]
    fn removing_one_duplicate_uses_its_position_instead_of_uri_as_key() {
        let before = playlist_fixture();
        let op = playlist_operation(
            &PlaylistEdit::Remove {
                position: 2,
                uri: "spotify:track:2cGxRwrMyEAp8dEbuZaVv6".into(),
            },
            &before,
        )
        .unwrap();
        assert_eq!(op.rem.as_ref().unwrap().from_index(), 2);
        assert_eq!(op.rem.as_ref().unwrap().length(), 1);
        assert!(!op.rem.as_ref().unwrap().items_as_key());
        assert!(matches!(
            playlist_operation(
                &PlaylistEdit::Remove {
                    position: 2,
                    uri: "spotify:track:2LD2gT7gwAurzdQDQtILds".into()
                },
                &before
            ),
            Err(LibraryError::Conflict)
        ));
    }
    #[test]
    fn moving_forward_accounts_for_the_pre_removal_insertion_index() {
        for (to, insertion) in [(1, 1), (3, 4)] {
            let op = playlist_operation(
                &PlaylistEdit::Move {
                    position: 2,
                    to,
                    uri: "spotify:track:2cGxRwrMyEAp8dEbuZaVv6".into(),
                },
                &playlist_fixture(),
            )
            .unwrap();
            assert_eq!(op.mov.as_ref().unwrap().to_index(), insertion);
        }
    }
    #[test]
    fn explicit_server_permissions_override_owner_fallback() {
        let mut before = playlist_fixture();
        assert_eq!(playlist_permissions(&before, "owner"), (true, true));
        assert_eq!(playlist_permissions(&before, "other"), (false, false));
        let mut caps = librespot_protocol::playlist_permission::Capabilities::new();
        caps.set_can_edit_items(false);
        caps.set_can_edit_metadata(false);
        before.capabilities = Some(caps).into();
        assert_eq!(playlist_permissions(&before, "owner"), (false, false));
    }
    #[test]
    fn revisions_and_text_inputs_are_checked_before_writes() {
        assert_eq!(
            revision_bytes(&revision_hex(&[0, 255, 10])).unwrap(),
            [0, 255, 10]
        );
        assert!(revision_bytes("é0").is_err());
        assert!(revision_bytes("z0").is_err());
        assert!(revision_bytes("").is_err());
        assert!(validate_details("  ", "").is_err());
        assert!(validate_details("Playlist", &"x".repeat(301)).is_err());
    }
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
            assert!(
                decode_contains(data, 2).is_err(),
                "accepted malformed frame: {data:?}"
            );
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
