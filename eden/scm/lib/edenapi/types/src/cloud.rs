/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;
use types::HgId;

use crate::ServerError;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct WorkspaceData {
    #[id(0)]
    pub name: String,
    #[id(1)]
    pub reponame: String,
    #[id(2)]
    pub version: u64,
    #[id(3)]
    pub archived: bool,
    #[id(4)]
    pub timestamp: i64,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CloudWorkspaceRequest {
    #[id(0)]
    pub workspace: String,
    #[id(1)]
    pub reponame: String,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CloudWorkspacesRequest {
    #[id(0)]
    pub prefix: String,
    #[id(1)]
    pub reponame: String,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct UpdateReferencesParams {
    #[id(0)]
    pub workspace: String,
    #[id(1)]
    pub reponame: String,
    #[id(2)]
    pub version: u64,
    #[id(3)]
    pub removed_heads: Vec<HgId>,
    #[id(4)]
    pub new_heads: Vec<HgId>,
    #[id(5)]
    pub updated_bookmarks: HashMap<String, HgId>,
    #[id(6)]
    pub removed_bookmarks: Vec<String>,
    #[id(7)]
    pub updated_remote_bookmarks: Option<Vec<RemoteBookmark>>,
    #[id(8)]
    pub removed_remote_bookmarks: Option<Vec<RemoteBookmark>>,
    #[id(9)]
    pub new_snapshots: Vec<HgId>,
    #[id(10)]
    pub removed_snapshots: Vec<HgId>,
    #[id(11)]
    pub client_info: Option<ClientInfo>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct GetReferencesParams {
    #[id(0)]
    pub workspace: String,
    #[id(1)]
    pub reponame: String,
    #[id(2)]
    pub version: u64,
    #[id(3)]
    pub client_info: Option<ClientInfo>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct ReferencesData {
    #[id(0)]
    pub version: u64,
    #[id(1)]
    pub heads: Option<Vec<HgId>>,
    #[id(2)]
    pub bookmarks: Option<HashMap<String, HgId>>,
    #[id(3)]
    pub heads_dates: Option<HashMap<HgId, i64>>,
    #[id(4)]
    pub remote_bookmarks: Option<Vec<RemoteBookmark>>,
    #[id(5)]
    pub snapshots: Option<Vec<HgId>>,
    #[id(6)]
    pub timestamp: Option<i64>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct RemoteBookmark {
    #[id(0)]
    pub remote: String,
    #[id(1)]
    pub name: String,
    #[id(2)]
    pub node: Option<HgId>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct ClientInfo {
    #[id(0)]
    pub hostname: String,
    #[id(1)]
    pub reporoot: String,
    #[id(2)]
    pub version: u64,
}

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct ReferencesDataResponse {
    #[id(0)]
    #[no_default]
    pub data: Result<ReferencesData, ServerError>,
}

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct WorkspaceDataResponse {
    #[id(0)]
    #[no_default]
    pub data: Result<WorkspaceData, ServerError>,
}

#[auto_wire]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct WorkspacesDataResponse {
    #[id(0)]
    #[no_default]
    pub data: Result<Vec<WorkspaceData>, ServerError>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct GetSmartlogParams {
    #[id(0)]
    pub workspace: String,
    #[id(1)]
    pub reponame: String,
    #[id(2)]
    pub flags: Vec<GetSmartlogFlag>,
}

#[auto_wire]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub enum GetSmartlogFlag {
    #[id(1)]
    SkipPublicCommitsMetadata,
    #[id(2)]
    AddRemoteBookmarks,
    #[id(3)]
    AddAllBookmarks,
}

// Wire requires a default value, shouldn't be used
impl Default for GetSmartlogFlag {
    fn default() -> Self {
        Self::AddAllBookmarks
    }
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct SmartlogNode {
    #[id(0)]
    pub node: HgId,
    #[id(1)]
    pub phase: String,
    #[id(2)]
    pub author: String,
    #[id(3)]
    pub date: i64,
    #[id(4)]
    pub message: String,
    #[id(5)]
    pub parents: Vec<HgId>,
    #[id(6)]
    pub bookmarks: Vec<String>,
    #[id(7)]
    pub remote_bookmarks: Option<Vec<RemoteBookmark>>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct SmartlogData {
    #[id(0)]
    pub nodes: Vec<SmartlogNode>,
    #[id(1)]
    pub version: i64,
    #[id(2)]
    pub timestamp: i64,
}

impl RemoteBookmark {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.remote, self.name)
    }
}
