use super::x11::X11;
use crate::session_options::SshOptions;
fn setup(little: bool, cookie: &[u8]) -> Vec<u8> {
    let mut bytes = vec![if little { b'l' } else { b'B' }, 0];
    for word in [11u16, 0, 18, cookie.len() as u16, 0] {
        bytes.extend(if little {
            word.to_le_bytes()
        } else {
            word.to_be_bytes()
        });
    }
    bytes.extend_from_slice(b"MIT-MAGIC-COOKIE-1");
    bytes.resize(32, 0);
    bytes.extend(cookie);
    bytes
}
#[test]
fn x11_authenticates_fragmented_both_endian_setups_and_never_sends_fake_cookie_locally() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Xauthority");
    let mut auth = 0u16.to_be_bytes().to_vec();
    for field in [&[127, 0, 0, 1][..], b"5", b"MIT-MAGIC-COOKIE-1", &[7u8; 16]] {
        auth.extend((field.len() as u16).to_be_bytes());
        auth.extend(field);
    }
    std::fs::write(&path, &auth).unwrap();
    let options = SshOptions {
        x11_display: "127.0.0.1:5.1".into(),
        x11_auth_file: path.to_string_lossy().into(),
        ..Default::default()
    };
    let x11 = X11::load(&options).unwrap();
    assert_eq!(x11.screen, 1);
    for little in [true, false] {
        let mut bytes = setup(little, &x11.fake);
        assert!(x11.setup(&bytes[..40]).unwrap().is_none());
        bytes.extend_from_slice(b"application request");
        let (used, local) = x11.setup(&bytes).unwrap().unwrap();
        assert_eq!(used, 48);
        assert_eq!(local, setup(little, &[7; 16]));
        bytes[32] ^= 1;
        assert!(x11.setup(&bytes).is_err());
        let mut oversized = setup(little, &x11.fake);
        oversized[6..8].copy_from_slice(&[255; 2]);
        assert!(x11.setup(&oversized).is_err());
    }
    let no_auth = X11::load(&SshOptions {
        x11_no_auth: true,
        ..options
    })
    .unwrap();
    let (_, local) = no_auth.setup(&setup(true, &no_auth.fake)).unwrap().unwrap();
    assert_eq!(local.len(), 12);
    assert_eq!(&local[6..10], &[0; 4]);
}
