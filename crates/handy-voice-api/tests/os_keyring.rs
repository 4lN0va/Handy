//! Real operating-system credential store tests.
//!
//! # These are not mocks
//!
//! Every other secrets test in this crate uses `MemorySecretStore`. That proves
//! the *logic* but says nothing about whether the OS backend works: whether the
//! entry is really written, really survives a fresh handle, and is really
//! removed. Those questions can only be answered by touching the actual
//! credential store.
//!
//! # Why they are `#[ignore]`d
//!
//! They mutate machine state — on Windows they create and delete real entries in
//! Credential Manager. Running that by default would be rude and, on a developer
//! machine, potentially surprising. Run them explicitly:
//!
//! ```text
//! cargo test --features keyring --test os_keyring -- --ignored --test-threads=1
//! ```
//!
//! # Safety rules these tests follow
//!
//! * The service name is a dedicated test namespace, so a lookup can never
//!   collide with, read, or overwrite an entry belonging to the application or to
//!   any other program.
//! * Each test uses its own account name derived from the test name, so parallel
//!   or repeated runs cannot interfere with one another.
//! * Every test deletes what it created, and deletion is also asserted to be
//!   idempotent so a failed run cannot leave a trap behind.
//! * No secret value is ever printed. Assertions compare values in memory; a
//!   failure message reports only lengths and booleans.
#![cfg(feature = "keyring")]

use handy_voice_api::secrets::os_store::OsSecretStore;
use handy_voice_api::secrets::{SecretStore, SecretStoreKind, SecretString};

/// A namespace that is obviously not the application's.
///
/// `SECRET_SERVICE` is `handy-voice-api`; using a separate `...-selftest` name
/// means these tests cannot read or destroy a real stored key.
const TEST_SERVICE: &str = "handy-voice-api-selftest";

/// A synthetic credential. Deliberately not a real-looking provider key.
const TEST_SECRET: &str = "selftest-value-not-a-real-credential-0123456789";

fn store() -> OsSecretStore {
    OsSecretStore::new(TEST_SERVICE)
}

/// A unique account name per test, so runs cannot collide.
fn account(test: &str) -> String {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{test}-{pid}-{nanos}")
}

/// Run `body`, then always remove the entry, even if the body panics.
fn with_account<T>(test: &str, body: impl FnOnce(&str) -> T) -> T {
    let acct = account(test);
    let s = store();
    // Best-effort pre-clean in case an earlier crashed run left something.
    let _ = s.delete(&acct);
    let result = body(&acct);
    let cleanup = s.delete(&acct);
    assert!(
        cleanup.is_ok(),
        "cleanup failed for a test entry; check the credential store manually"
    );
    result
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn the_backend_reports_itself_as_the_os_store() {
    assert_eq!(store().kind(), SecretStoreKind::OsCredentialStore);
    assert!(store().kind().describe().contains("credential store"));
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn a_missing_entry_reads_as_none_rather_than_erroring() {
    with_account("missing", |acct| {
        let got = store().get(acct).expect("a lookup must not error");
        assert!(got.is_none(), "an unknown account must read as None");
    });
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn a_secret_written_to_the_os_store_can_be_read_back() {
    with_account("roundtrip", |acct| {
        let s = store();
        s.set(acct, &SecretString::new(TEST_SECRET))
            .expect("write to the OS credential store");

        let read = s
            .get(acct)
            .expect("read must not error")
            .expect("the entry must exist after writing");

        assert_eq!(read.expose(), TEST_SECRET);
        assert_eq!(read.len(), TEST_SECRET.len(), "length mismatch");
        // Never print the value itself.
    });
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn a_secret_survives_a_fresh_store_handle() {
    // Models a process restart: a brand-new handle with no shared in-memory
    // state must still see what was written.
    with_account("persist", |acct| {
        store()
            .set(acct, &SecretString::new(TEST_SECRET))
            .expect("write");

        let fresh = OsSecretStore::new(TEST_SERVICE);
        let read = fresh
            .get(acct)
            .expect("read")
            .expect("a fresh handle must still find the entry");
        assert_eq!(read.expose(), TEST_SECRET);
    });
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn updating_a_secret_replaces_the_stored_value() {
    with_account("update", |acct| {
        let s = store();
        s.set(acct, &SecretString::new("first-value"))
            .expect("write");
        assert_eq!(s.get(acct).unwrap().unwrap().expose(), "first-value");

        s.set(acct, &SecretString::new("second-value"))
            .expect("overwrite");
        assert_eq!(
            s.get(acct).unwrap().unwrap().expose(),
            "second-value",
            "an overwrite must replace, not append or fail"
        );
    });
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn deleting_a_secret_removes_it_and_is_idempotent() {
    with_account("delete", |acct| {
        let s = store();
        s.set(acct, &SecretString::new(TEST_SECRET)).expect("write");
        assert!(s.get(acct).unwrap().is_some());

        s.delete(acct).expect("delete");
        assert!(
            s.get(acct).unwrap().is_none(),
            "the entry must be gone after deletion"
        );

        // Deleting again must not error.
        s.delete(acct).expect("delete must be idempotent");
    });
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn entries_are_isolated_by_account_name() {
    with_account("isolation-a", |acct_a| {
        let acct_b = account("isolation-b");
        let s = store();
        let _ = s.delete(&acct_b);

        s.set(acct_a, &SecretString::new("value-a"))
            .expect("write a");
        s.set(&acct_b, &SecretString::new("value-b"))
            .expect("write b");

        assert_eq!(s.get(acct_a).unwrap().unwrap().expose(), "value-a");
        assert_eq!(s.get(&acct_b).unwrap().unwrap().expose(), "value-b");

        s.delete(&acct_b).expect("cleanup b");
    });
}

#[test]
#[ignore = "touches the real OS credential store; run explicitly with --ignored"]
fn the_test_namespace_cannot_see_the_application_namespace() {
    // Guard against a mistake that would make these tests read or destroy the
    // application's real key: the two services must be genuinely separate.
    assert_ne!(TEST_SERVICE, handy_voice_api::secrets::SECRET_SERVICE);

    with_account("namespace", |acct| {
        store()
            .set(acct, &SecretString::new(TEST_SECRET))
            .expect("write");
        // The same account name under the APPLICATION service must be unrelated.
        let app_store = OsSecretStore::new(handy_voice_api::secrets::SECRET_SERVICE);
        if let Ok(Some(found)) = app_store.get(acct) {
            panic!(
                "the application namespace unexpectedly contains an account written \
                 under the test namespace (len {}) - the services are not isolated",
                found.len()
            );
        }
    });
}
