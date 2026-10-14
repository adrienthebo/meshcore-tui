# Changelog

## Unreleased

### Added

- **Tabs:** the UI is split into Overview, Contacts, Radio and RX Log tabs,
  switched with F1-F9 or Alt and the tab's number. Keys go to the focused
  panel first, and the tab bar shows the keys that work right now.
  (198541c, 245aa31, c049295, 00d3b81, 7753af0, 4fdcd4e)
- **Contacts tab:** a table of contacts with their type, key, advert age, hops
  and position, most recently heard first, above the adverts the radio
  reported. Enter on a contact addresses messages to it. Advert ages are
  shown to the minute in their two largest units, and an advert from a clock
  ahead of ours shows a negative age. The browsed contact and the scrolled
  adverts stay put as contacts and adverts arrive. (4f19087, 93d3d5c,
  3fa9ded, 006c663, 2498075, 7c59957, 4ce05a4)
- **Discovered nodes:** nodes the radio heard but didn't store as contacts
  are listed under Discovered on the Contacts tab, with when we heard them,
  instead of among the contacts. `a` on one adds it as a contact; a full
  contact table is reported as such. (6258e4c, c262d98)
- **Contact management:** add contacts by name and public key with
  `/contact-add`, from the MeshCore app's `meshcore://contact/add` links, or
  from `meshcore://` biz cards. Delete them by name or public key with
  `/contact-del`, which refuses a name that matches several contacts.
  (72b643c, e83f861, 215fe65, 35835c7)
- **Radio diagnostics:** the radio's stats are polled every 5 seconds. The
  Overview's Radio panel graphs the noise floor and shows the last packet's
  RSSI and SNR and the airtime in its three largest units, from weeks to
  seconds.
  (c2dc2e1, 6264cfe, 9758448, b25acd6, 7fbab10)
- **Radio tab:** 20-minute charts of the noise floor and the SNR of every
  received packet, plus signal and TX queue, airtime as a share of uptime,
  flood and direct packet counts, receive errors, error flags, battery,
  storage, clock drift, TX power, firmware, model and location. The extra
  readings are polled only while the tab is on screen. (c66ae8d, 2d08d8c,
  0152048, 08e18b4, d2e30c1, ace1e38, d770265, e220903, 45dd868)
- **RX Log tab:** the last 5,000 packets the radio heard, with their signal,
  type, route, hops and length. The selected packet is decoded into fields
  with a hex dump; channel messages and adverts are broken into their parts.
  It follows the newest packet until you browse one, and `p` pauses it.
  (415f2b3, 2709b2b, 718c421, 3268d68, 5975332, 188de5a, 3a3c1d6)
- **Activity panel** (renamed from Received Packets): entry kinds are spelled
  out (Advert, Ack, Path, Sent) and line up. Advert and route updates name the
  contact, and a new contact's row shows its route. (174ce37, c3ef3da,
  509612f, 9c64152, 3d928dc, 5dc3fe2)
- **Themes:** colours come from a theme, selected with `--theme`. Vaporwave is
  the default; `classic` uses the terminal's named colours. RSSI, SNR, noise
  floor, packet length and hop hash size are coloured by quality. Panels have
  rounded borders and bold titles. (9edf962, 5783bfa, 69daa1a, f8a493e,
  d0bef00, 7ba1d18, 888fb91, 701f524)
- **Message input:** stops at the radio's 160-byte message limit, which
  includes the `name: ` prefix on channels, and counts the bytes left.
  (98ca222)
- **Releases:** binaries for macOS arm64 and Linux arm64 and amd64 (static
  musl) are attached to a draft GitHub release when a version tag is pushed.
  Building from source needs Rust 1.89 or newer. (f90e477, b7b15d8)
- **BLE build feature:** BLE support is built only with `--features ble`.
  Release binaries connect over serial or TCP; their `--ble` reports that BLE
  isn't built in. (c89eb0c, 11a02d1)

### Fixed

- The screen froze while a radio command ran; commands now run in the
  background, so it keeps redrawing and taking keys. (e1cd522)

- `/setch` generated channel secrets from the clock, giving about 256
  possible keys; it now uses the OS random source. It also rejects secrets
  that aren't exactly 16 bytes instead of truncating them. (2bbf7cb, 72958ae)
- Typing or deleting non-ASCII text no longer panics, and the cursor sits in
  the right column. (fd9a146)
- Emoji and CJK text in names and messages displays and wraps correctly.
  Invisible and bidi characters are stripped, including from our own sent
  messages. (cd635c5, 610932b, b26bb2d)
- The DM target is held by public key rather than list position, so
  reloading contacts no longer retargets the next message. (48f5b43)
- Hop counts for routes with 2- and 3-byte hashes showed as 64 or more, or
  as 0; flood routes showed 255. (5dc3fe2, f16c118, 31aaad7)
- Route update pushes were misparsed and now name the right contact.
  (9c64152)
- `/ch` with a missing or non-numeric index reports an error, and radio
  commands report "Not connected" when offline instead of doing nothing.
  (f969464)
- The message log grew without bound and sent messages skipped the activity
  log's cap; they now keep the last 1,000 messages and 100 entries.
  (31aaad7)
