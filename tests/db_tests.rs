use caldav_ics_sync::db::*;
use rusqlite::Connection;

fn setup() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    init_db(&conn).unwrap();
    conn
}

fn valid_source() -> CreateSource {
    CreateSource {
        name: "Test".into(),
        caldav_url: "https://cal.example.com".into(),
        username: "user".into(),
        password: "pass".into(),
        ics_path: "cal.ics".into(),
        sync_interval_secs: 3600,
        public_ics: false,
        public_ics_path: None,
    }
}

fn valid_destination() -> CreateDestination {
    CreateDestination {
        name: "Dest".into(),
        ics_url: "https://example.com/cal.ics".into(),
        caldav_url: "https://caldav.example.com".into(),
        calendar_name: "main".into(),
        username: "user".into(),
        password: "pass".into(),
        sync_interval_secs: 3600,
        sync_all: false,
        keep_local: false,
    }
}

// ---- Sources CRUD ----

#[test]
fn create_source_valid() {
    let conn = setup();
    let id = create_source(&conn, &valid_source()).unwrap();
    assert!(id > 0);
}

#[test]
fn create_source_rejects_empty_name() {
    let conn = setup();
    let mut s = valid_source();
    s.name = "  ".into();
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_empty_caldav_url() {
    let conn = setup();
    let mut s = valid_source();
    s.caldav_url = "".into();
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_empty_username() {
    let conn = setup();
    let mut s = valid_source();
    s.username = "".into();
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_empty_password() {
    let conn = setup();
    let mut s = valid_source();
    s.password = "".into();
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_empty_ics_path() {
    let conn = setup();
    let mut s = valid_source();
    s.ics_path = "  ".into();
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_negative_sync_interval() {
    let conn = setup();
    let mut s = valid_source();
    s.sync_interval_secs = -1;
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_duplicate_ics_path() {
    let conn = setup();
    create_source(&conn, &valid_source()).unwrap();
    let mut s2 = valid_source();
    s2.name = "Other".into();
    assert!(create_source(&conn, &s2).is_err());
}

#[test]
fn create_source_rejects_public_ics_path_prefix() {
    let conn = setup();
    let mut s = valid_source();
    s.ics_path = "public".into();
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_public_slash_prefix() {
    let conn = setup();
    let mut s = valid_source();
    s.ics_path = "public/foo".into();
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn list_sources_returns_created() {
    let conn = setup();
    create_source(&conn, &valid_source()).unwrap();

    let mut s2 = valid_source();
    s2.ics_path = "other.ics".into();
    create_source(&conn, &s2).unwrap();

    let sources = list_sources(&conn).unwrap();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].ics_path, "cal.ics");
    assert_eq!(sources[1].ics_path, "other.ics");
}

#[test]
fn get_source_by_id() {
    let conn = setup();
    let id = create_source(&conn, &valid_source()).unwrap();
    let src = get_source(&conn, id).unwrap().unwrap();
    assert_eq!(src.name, "Test");
    assert_eq!(src.ics_path, "cal.ics");
}

#[test]
fn get_source_nonexistent() {
    let conn = setup();
    assert!(get_source(&conn, 999).unwrap().is_none());
}

#[test]
fn update_source_preserves_password_on_empty() {
    let conn = setup();
    let id = create_source(&conn, &valid_source()).unwrap();
    let upd = UpdateSource {
        name: Some("Renamed".into()),
        caldav_url: None,
        username: None,
        password: Some("".into()),
        ics_path: None,
        sync_interval_secs: None,
        public_ics: None,
        public_ics_path: None,
    };
    update_source(&conn, id, &upd).unwrap();
    let src = get_source(&conn, id).unwrap().unwrap();
    assert_eq!(src.name, "Renamed");
    assert_eq!(src.password, "pass");
}

#[test]
fn update_source_rejects_duplicate_ics_path() {
    let conn = setup();
    let id1 = create_source(&conn, &valid_source()).unwrap();

    let mut s2 = valid_source();
    s2.ics_path = "other.ics".into();
    create_source(&conn, &s2).unwrap();

    let upd = UpdateSource {
        name: None,
        caldav_url: None,
        username: None,
        password: None,
        ics_path: Some("other.ics".into()),
        sync_interval_secs: None,
        public_ics: None,
        public_ics_path: None,
    };
    assert!(update_source(&conn, id1, &upd).is_err());
}

#[test]
fn delete_source_removes_it() {
    let conn = setup();
    let id = create_source(&conn, &valid_source()).unwrap();
    assert!(delete_source(&conn, id).unwrap());
    assert!(get_source(&conn, id).unwrap().is_none());
}

#[test]
fn delete_source_nonexistent() {
    let conn = setup();
    assert!(!delete_source(&conn, 999).unwrap());
}

// ---- Public ICS ----

#[test]
fn create_source_public_with_custom_path() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("shared/cal.ics".into());
    let id = create_source(&conn, &s).unwrap();
    let src = get_source(&conn, id).unwrap().unwrap();
    assert!(src.public_ics);
    assert_eq!(src.public_ics_path.as_deref(), Some("shared/cal.ics"));
}

#[test]
fn create_source_public_no_custom_path() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = None;
    let id = create_source(&conn, &s).unwrap();
    let src = get_source(&conn, id).unwrap().unwrap();
    assert!(src.public_ics);
    assert!(src.public_ics_path.is_none());
}

#[test]
fn create_source_rejects_public_path_with_dotdot() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("foo/../bar".into());
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_public_path_with_leading_slash() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("/foo/bar".into());
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn create_source_rejects_public_path_same_as_ics_path() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("cal.ics".into());
    assert!(create_source(&conn, &s).is_err());
}

#[test]
fn cross_table_ics_path_cannot_match_another_public_path() {
    let conn = setup();
    let mut s1 = valid_source();
    s1.public_ics = true;
    s1.public_ics_path = Some("shared.ics".into());
    create_source(&conn, &s1).unwrap();

    let mut s2 = valid_source();
    s2.ics_path = "shared.ics".into();
    assert!(create_source(&conn, &s2).is_err());
}

#[test]
fn cross_table_public_path_cannot_match_another_ics_path() {
    let conn = setup();
    create_source(&conn, &valid_source()).unwrap();

    let mut s2 = valid_source();
    s2.ics_path = "other.ics".into();
    s2.public_ics = true;
    s2.public_ics_path = Some("cal.ics".into());
    assert!(create_source(&conn, &s2).is_err());
}

#[test]
fn update_public_ics_false_clears_public_path() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("shared.ics".into());
    let id = create_source(&conn, &s).unwrap();

    let upd = UpdateSource {
        name: None,
        caldav_url: None,
        username: None,
        password: None,
        ics_path: None,
        sync_interval_secs: None,
        public_ics: Some(false),
        public_ics_path: None,
    };
    update_source(&conn, id, &upd).unwrap();
    let src = get_source(&conn, id).unwrap().unwrap();
    assert!(!src.public_ics);
    assert!(src.public_ics_path.is_none());
}

#[test]
fn get_ics_data_by_public_path_only_when_public() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("shared.ics".into());
    let id = create_source(&conn, &s).unwrap();
    save_ics_data(&conn, id, "BEGIN:VCALENDAR\nEND:VCALENDAR").unwrap();

    let data = get_ics_data_by_public_path(&conn, "shared.ics").unwrap();
    assert!(data.is_some());

    let upd = UpdateSource {
        name: None,
        caldav_url: None,
        username: None,
        password: None,
        ics_path: None,
        sync_interval_secs: None,
        public_ics: Some(false),
        public_ics_path: None,
    };
    update_source(&conn, id, &upd).unwrap();
    let data = get_ics_data_by_public_path(&conn, "shared.ics").unwrap();
    assert!(data.is_none());
}

#[test]
fn is_public_standard_ics_true_when_public_no_custom_path() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = None;
    let id = create_source(&conn, &s).unwrap();
    save_ics_data(&conn, id, "data").unwrap();

    assert!(is_public_standard_ics(&conn, "cal.ics").unwrap());
}

#[test]
fn is_public_standard_ics_false_when_custom_path() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("shared.ics".into());
    create_source(&conn, &s).unwrap();

    assert!(!is_public_standard_ics(&conn, "cal.ics").unwrap());
}

#[test]
fn is_public_standard_ics_false_when_not_public() {
    let conn = setup();
    create_source(&conn, &valid_source()).unwrap();
    assert!(!is_public_standard_ics(&conn, "cal.ics").unwrap());
}

// ---- Source Paths ----

#[test]
fn create_source_path_succeeds() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let body = CreateSourcePath {
        path: "alias.ics".into(),
        is_public: false,
    };
    let sp_id = create_source_path(&conn, src_id, &body).unwrap();
    assert!(sp_id > 0);
}

#[test]
fn create_source_path_rejects_duplicate() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let body = CreateSourcePath {
        path: "alias.ics".into(),
        is_public: false,
    };
    create_source_path(&conn, src_id, &body).unwrap();
    assert!(create_source_path(&conn, src_id, &body).is_err());
}

#[test]
fn create_source_path_rejects_match_with_sources_ics_path() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let body = CreateSourcePath {
        path: "cal.ics".into(),
        is_public: false,
    };
    assert!(create_source_path(&conn, src_id, &body).is_err());
}

#[test]
fn create_source_path_rejects_match_with_sources_public_path() {
    let conn = setup();
    let mut s = valid_source();
    s.public_ics = true;
    s.public_ics_path = Some("shared.ics".into());
    let src_id = create_source(&conn, &s).unwrap();
    let body = CreateSourcePath {
        path: "shared.ics".into(),
        is_public: false,
    };
    assert!(create_source_path(&conn, src_id, &body).is_err());
}

#[test]
fn create_source_path_rejects_public_prefix() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let body = CreateSourcePath {
        path: "public/foo".into(),
        is_public: false,
    };
    assert!(create_source_path(&conn, src_id, &body).is_err());
}

#[test]
fn create_source_path_rejects_public_exact() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let body = CreateSourcePath {
        path: "public".into(),
        is_public: false,
    };
    assert!(create_source_path(&conn, src_id, &body).is_err());
}

#[test]
fn create_source_path_rejects_dotdot() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let body = CreateSourcePath {
        path: "foo/../bar".into(),
        is_public: false,
    };
    assert!(create_source_path(&conn, src_id, &body).is_err());
}

#[test]
fn create_source_path_rejects_leading_slash() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let body = CreateSourcePath {
        path: "/foo.ics".into(),
        is_public: false,
    };
    assert!(create_source_path(&conn, src_id, &body).is_err());
}

#[test]
fn list_source_paths_for_source() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "a.ics".into(),
            is_public: false,
        },
    )
    .unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "b.ics".into(),
            is_public: true,
        },
    )
    .unwrap();

    let paths = list_source_paths(&conn, src_id).unwrap();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0].path, "a.ics");
    assert_eq!(paths[1].path, "b.ics");
}

#[test]
fn update_source_path_changes_path() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let sp_id = create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "old.ics".into(),
            is_public: false,
        },
    )
    .unwrap();
    let upd = UpdateSourcePath {
        path: Some("new.ics".into()),
        is_public: None,
    };
    assert!(update_source_path(&conn, sp_id, &upd).unwrap());
    let sp = get_source_path(&conn, sp_id).unwrap().unwrap();
    assert_eq!(sp.path, "new.ics");
}

#[test]
fn delete_source_path_removes_it() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let sp_id = create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "alias.ics".into(),
            is_public: false,
        },
    )
    .unwrap();
    assert!(delete_source_path(&conn, sp_id).unwrap());
    assert!(get_source_path(&conn, sp_id).unwrap().is_none());
}

#[test]
fn get_ics_data_by_path_finds_via_source_paths() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    save_ics_data(&conn, src_id, "ICS_CONTENT").unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "alias.ics".into(),
            is_public: false,
        },
    )
    .unwrap();

    let data = get_ics_data_by_path(&conn, "alias.ics").unwrap();
    assert_eq!(data.as_deref(), Some("ICS_CONTENT"));
}

#[test]
fn get_ics_data_by_public_path_finds_via_source_paths() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    save_ics_data(&conn, src_id, "PUB_DATA").unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "pub-alias.ics".into(),
            is_public: true,
        },
    )
    .unwrap();

    let data = get_ics_data_by_public_path(&conn, "pub-alias.ics").unwrap();
    assert_eq!(data.as_deref(), Some("PUB_DATA"));
}

#[test]
fn get_ics_data_by_public_path_not_found_when_not_public() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    save_ics_data(&conn, src_id, "DATA").unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "priv.ics".into(),
            is_public: false,
        },
    )
    .unwrap();

    let data = get_ics_data_by_public_path(&conn, "priv.ics").unwrap();
    assert!(data.is_none());
}

#[test]
fn is_public_standard_ics_via_source_paths() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "std-pub.ics".into(),
            is_public: true,
        },
    )
    .unwrap();
    assert!(is_public_standard_ics(&conn, "std-pub.ics").unwrap());
}

#[test]
fn is_public_standard_ics_false_for_private_source_path() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "priv.ics".into(),
            is_public: false,
        },
    )
    .unwrap();
    assert!(!is_public_standard_ics(&conn, "priv.ics").unwrap());
}

#[test]
fn source_paths_deleted_on_cascade_when_source_deleted() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    let sp_id = create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "alias.ics".into(),
            is_public: false,
        },
    )
    .unwrap();
    delete_source(&conn, src_id).unwrap();
    assert!(get_source_path(&conn, sp_id).unwrap().is_none());
}

// ---- Destinations CRUD ----

#[test]
fn create_destination_valid() {
    let conn = setup();
    let id = create_destination(&conn, &valid_destination()).unwrap();
    assert!(id > 0);
}

#[test]
fn create_destination_rejects_empty_name() {
    let conn = setup();
    let mut d = valid_destination();
    d.name = "".into();
    assert!(create_destination(&conn, &d).is_err());
}

#[test]
fn create_destination_rejects_empty_ics_url() {
    let conn = setup();
    let mut d = valid_destination();
    d.ics_url = "".into();
    assert!(create_destination(&conn, &d).is_err());
}

#[test]
fn create_destination_rejects_empty_caldav_url() {
    let conn = setup();
    let mut d = valid_destination();
    d.caldav_url = "".into();
    assert!(create_destination(&conn, &d).is_err());
}

#[test]
fn create_destination_rejects_empty_calendar_name() {
    let conn = setup();
    let mut d = valid_destination();
    d.calendar_name = "".into();
    assert!(create_destination(&conn, &d).is_err());
}

#[test]
fn create_destination_rejects_empty_username() {
    let conn = setup();
    let mut d = valid_destination();
    d.username = "".into();
    assert!(create_destination(&conn, &d).is_err());
}

#[test]
fn create_destination_rejects_empty_password() {
    let conn = setup();
    let mut d = valid_destination();
    d.password = "".into();
    assert!(create_destination(&conn, &d).is_err());
}

#[test]
fn update_destination_preserves_password_on_empty() {
    let conn = setup();
    let id = create_destination(&conn, &valid_destination()).unwrap();
    let upd = UpdateDestination {
        name: Some("Renamed".into()),
        ics_url: None,
        caldav_url: None,
        calendar_name: None,
        username: None,
        password: Some("".into()),
        sync_interval_secs: None,
        sync_all: None,
        keep_local: None,
    };
    update_destination(&conn, id, &upd).unwrap();
    let dest = get_destination(&conn, id).unwrap().unwrap();
    assert_eq!(dest.name, "Renamed");
    assert_eq!(dest.password, "pass");
}

#[test]
fn delete_destination_removes_it() {
    let conn = setup();
    let id = create_destination(&conn, &valid_destination()).unwrap();
    assert!(delete_destination(&conn, id).unwrap());
    assert!(get_destination(&conn, id).unwrap().is_none());
}

#[test]
fn delete_destination_nonexistent() {
    let conn = setup();
    assert!(!delete_destination(&conn, 999).unwrap());
}

// ---- Overlapping Destinations ----

#[test]
fn find_overlapping_destinations_returns_other() {
    let conn = setup();
    let id1 = create_destination(&conn, &valid_destination()).unwrap();
    let mut d2 = valid_destination();
    d2.name = "Dest2".into();
    d2.ics_url = "https://example.com/other.ics".into();
    let id2 = create_destination(&conn, &d2).unwrap();

    let overlaps =
        find_overlapping_destinations(&conn, "https://caldav.example.com", "main", None).unwrap();
    assert_eq!(overlaps.len(), 2);
    assert!(overlaps.iter().any(|d| d.id == id1));
    assert!(overlaps.iter().any(|d| d.id == id2));
}

#[test]
fn find_overlapping_destinations_exclude_id() {
    let conn = setup();
    let id1 = create_destination(&conn, &valid_destination()).unwrap();
    let mut d2 = valid_destination();
    d2.name = "Dest2".into();
    d2.ics_url = "https://example.com/other.ics".into();
    let id2 = create_destination(&conn, &d2).unwrap();

    let overlaps =
        find_overlapping_destinations(&conn, "https://caldav.example.com", "main", Some(id1))
            .unwrap();
    assert_eq!(overlaps.len(), 1);
    assert_eq!(overlaps[0].id, id2);
}

#[test]
fn find_overlapping_destinations_no_match() {
    let conn = setup();
    create_destination(&conn, &valid_destination()).unwrap();

    let overlaps =
        find_overlapping_destinations(&conn, "https://caldav.example.com", "other-calendar", None)
            .unwrap();
    assert!(overlaps.is_empty());
}

// ---- ICS Data ----

#[test]
fn save_and_retrieve_ics_data_by_path() {
    let conn = setup();
    let id = create_source(&conn, &valid_source()).unwrap();
    save_ics_data(&conn, id, "BEGIN:VCALENDAR\nEND:VCALENDAR").unwrap();

    let data = get_ics_data_by_path(&conn, "cal.ics").unwrap();
    assert_eq!(data.as_deref(), Some("BEGIN:VCALENDAR\nEND:VCALENDAR"));
}

#[test]
fn save_ics_data_upserts() {
    let conn = setup();
    let id = create_source(&conn, &valid_source()).unwrap();
    save_ics_data(&conn, id, "first").unwrap();
    save_ics_data(&conn, id, "second").unwrap();

    let data = get_ics_data(&conn, id).unwrap();
    assert_eq!(data.as_deref(), Some("second"));
}

#[test]
fn get_ics_data_by_path_not_found() {
    let conn = setup();
    assert!(
        get_ics_data_by_path(&conn, "missing.ics")
            .unwrap()
            .is_none()
    );
}

// ---- Cross-table: source_paths vs create_source ----

#[test]
fn create_source_rejects_ics_path_matching_existing_source_path() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "taken.ics".into(),
            is_public: false,
        },
    )
    .unwrap();

    let mut s2 = valid_source();
    s2.ics_path = "taken.ics".into();
    assert!(create_source(&conn, &s2).is_err());
}

#[test]
fn create_source_rejects_public_path_matching_existing_source_path() {
    let conn = setup();
    let src_id = create_source(&conn, &valid_source()).unwrap();
    create_source_path(
        &conn,
        src_id,
        &CreateSourcePath {
            path: "taken.ics".into(),
            is_public: false,
        },
    )
    .unwrap();

    let mut s2 = valid_source();
    s2.ics_path = "other2.ics".into();
    s2.public_ics = true;
    s2.public_ics_path = Some("taken.ics".into());
    assert!(create_source(&conn, &s2).is_err());
}
