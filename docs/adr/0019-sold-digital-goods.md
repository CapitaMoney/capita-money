# ADR 0019 – Sold digital goods: protection by identity, not by locks

Status: accepted for the alpha (2026-11-16; rules diary 27 and 28: owner decisions of 2026-11-14 and 2026-11-16; concept `P04.8 DRM concept v0.1`, design by Claude under the owner's rule "always the best solution"). New crates `capita-goods` (rules, pure, `no_std`) and `capita-marks` (personal copies and fingerprints of files). New section `[goods]` in the rules files.

## Context
The market sells digital goods for Capita first (diary 7): e-books, PDFs, courses. A digital good is worth something only if it cannot be copied freely, and only if nobody sells a work of somebody else. Hard DRM (encryption, device binding, special readers) annoys buyers, locks them to one vendor, is cracked anyway, and does not fit an AGPL project.

Capita has what an ordinary shop lacks: one verified person per account. A mark in a copy points to a real person whose account carries an income – a much stronger deterrent than an e-mail address.

## Decision
**Five layers** (concept L1–L5):
1. **Seller declaration and report button** – on every listing and in every copy's notice page (DSA Art. 16 notice and action). Not built here (market service, app).
2. **Work registry, first registration wins** (`capita_goods::registry`). Each upload is fingerprinted (`capita_marks::fingerprint`): SHA-256 of the file and a **text print** – the words of the text, small letters, in overlapping runs of 5, MinHash with 128 values. The same file, or a print at least `goods.match_threshold` (50 %) alike, of another seller → the listing is held, the seller asked, the earlier registrant told. A new edition of one's own work is no conflict. The print is fixed for all time (tests hold golden values), so a print stored on the chain stays comparable. First registration is evidence, not a verdict: older dated public evidence beats it; in the alpha the board decides.
3. **Personal copy** (`capita_marks::personalise`). Each download carries a mark: 8 bytes, HMAC-SHA-256 of the sale with a secret of the market service – **no name, no account in the file** (GDPR, data minimisation); only the service's records turn it into a sale. 2 check bytes refuse a damaged or forged mark. Several layers, so that removing one is not enough:
   - EPUB: package metadata, an empty element `cpm-…` in the first chapter, and **zero-width frames** in the running text of every chapter (after a space, positions keyed by the mark, at least one per chapter) – they survive conversion, copy and paste.
   - PDF: document information, and invisible text (render mode 3) on every page; the page content itself runs untouched inside `q … Q`.
   - Plain text: zero-width frames.
   - Visible: one line, "Personal copy, sold via Capita Money." The buyer reads one sentence at download: "Your copy is personal. Sharing it can be traced to your account." (rule 16: no ids, no hashes on screen.)
   Every byte of the book that is not an insertion stays as the seller made it; an EPUB keeps `mimetype` first and stored.
4. **Mark reader** (`capita_marks::read`): every layer, a vote over the payloads found; two different payloads = copies mixed, each one evidence.
5. **Sanction ladder** (`capita_goods::sanction`, diary 27 and 28):

| Position | Step | Effect |
|---|---|---|
| 1 | warning | quick message; the seller is compensated |
| 2 | selling ban, 1 calendar month | no selling: illegal products taken down, own sales frozen, legal products stay in the shop but cannot be sold |
| 3 and on | full ban, 1 calendar month each | also no buying, no Capita movements (send, receive, pay in person, euro door), no chat, video chat or social network |

- "A month" is the calendar month: the 17th to the 17th (`capita_goods::calendar`); a day the month lacks becomes its last day.
- Bans add up: a new ban starts when the running one ends; a full ban covers selling as long as it runs.
- **Never banned:** seeing everything (the shop, the realm, the own account), objecting in the app (reason and details attached, a text field), verifying again, receiving the Capita of one's age, **voting in glassroots** (a vote carries no message, so no tips; owner 2026-11-16).
- Two years without a violation (`goods.reset_after`) and the ladder starts again.
- The seller of a stolen work (rule 28) skips the warning: position 2 at least.
- **Compensation** at every violation, the warning included: 1 × price from the leaker's balance only, never below zero; the rest lapses (rule 10, no debt – never taken from future Capita).
- **Sellers see only a trust level** (ok / reduced), never the record: no public pillory in a real-name system.
- Earnings of a newly registered work are held 14 days (`goods.earnings_hold`), so a compensation is still possible. Downloads per sale: 8. Gift: built, off in the alpha. Resale: off.

All numbers stand in `[goods]` of the rules file; `capita_rules::<set>::goods()` hands them to `capita-goods`, checked when the crate is built.

## Consequences
+ The owner's ladder is the code, with one length to store (one month) and two steps of severity after the warning.
+ A leaked copy names its sale even after metadata and hidden element are removed, and in text pasted out of the book.
+ The same work in another format (EPUB, PDF, text) gives nearly the same print: re-uploading a known work is caught.
+ Pure rules, `no_std`, SCALE encodable: the chain can keep records and enforce bans with the same code.
− A skilled attacker can strip every mark (normalising Unicode removes the frames; re-typesetting a PDF removes its text). Accepted: the aim is deterrence and tracing (concept L3).
− Comparing two copies shows where the marks are. Keyed positions make it more work, not impossible.
− The text print needs text: scanned PDFs, images, audio and video get only the exact hash for now.
- **Not built (next):**
  - the market service (`services/market`: upload, registry, downloads, notice form, objection) and the screens;
  - **on chain**: the public work registry (hash, print, seller, time; no sales) and the bans, so that a full ban stops Capita movements in `pallet-capita` – a pallet of its own, after the open chain changes (PR 27) are merged;
  - courses and video (A/B segment marking), audio, images, C2PA content credentials, P2P distribution (ports only: `ContentStore`, `Personaliser`, `MarkReader`, `Fingerprinter`, `WorkRegistry`).
- Offene Frage: Q4, the decider – Verein board until 1000 verified accounts, then a jury of 5 random verified users, no reward in the alpha (the default) – owner: to discuss separately in plain English.
- Offene Frage: who pays the finder reward of rule 28 (built: the amount, 1 × price).
- Offene Frage: `goods.match_threshold` 50 % and `goods.max_downloads` 8 are Claude's defaults.
- Vermutung (Claude, ungeprüft): the small-enterprise exemption (DSA Art. 19) frees an e.V. from the complaint system and trader checks; Art. 16/17 apply anyway (legal check P04.3). The mark ↔ sale map is personal data: privacy notice and deletion period needed (P04.3).
