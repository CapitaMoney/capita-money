// SPDX-License-Identifier: AGPL-3.0-only
//! The goods rules as the owner decided them (rules diary 27, 28).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use capita_core::types::{AccountId, Amount, Date, Duration, Share, Timestamp};
use capita_goods::calendar::add_months;
use capita_goods::compensation::{compensation, finder_reward};
use capita_goods::fingerprint::{word_hashes, ContentHash, Fingerprint, TextPrint, PRINT_LEN};
use capita_goods::licence::{earnings_free_from, Licence};
use capita_goods::registry::{judge, Decision, Match, Work};
use capita_goods::sanction::{Action, Offence, Record, Step, Trust};
use capita_goods::{GoodsError, GoodsParams};

fn alpha() -> GoodsParams {
    capita_rules::alpha::goods().unwrap()
}

/// 10:00 UTC on the given day.
fn at(y: u16, m: u8, d: u8) -> Timestamp {
    Date::new(y, m, d)
        .unwrap()
        .start()
        .unwrap()
        .checked_add(Duration::from_secs(10 * 3600))
        .unwrap()
}

fn account(n: u8) -> AccountId {
    AccountId([n; 32])
}

#[test]
fn every_rule_set_has_valid_goods_rules() {
    for set in capita_rules::SETS {
        let g = match *set {
            "alpha" => capita_rules::alpha::goods(),
            "dev-fast" => capita_rules::dev_fast::goods(),
            other => panic!("unknown set {other}"),
        };
        g.unwrap().validate().unwrap();
    }
}

#[test]
fn a_month_is_the_17th_to_the_17th() {
    assert_eq!(add_months(at(2026, 11, 17), 1).unwrap(), at(2026, 12, 17));
    assert_eq!(add_months(at(2026, 12, 17), 1).unwrap(), at(2027, 1, 17));
    // a day the month lacks becomes its last day
    assert_eq!(add_months(at(2027, 1, 31), 1).unwrap(), at(2027, 2, 28));
    assert_eq!(add_months(at(2028, 1, 31), 1).unwrap(), at(2028, 2, 29));
    assert_eq!(add_months(at(2026, 3, 31), 1).unwrap(), at(2026, 4, 30));
    assert_eq!(add_months(at(2026, 5, 2), 12).unwrap(), at(2027, 5, 2));
}

#[test]
fn the_ladder_warns_then_bans_selling_then_everything() {
    let p = alpha();
    let mut r = Record::CLEAN;
    let first = r.apply(&p, Offence::LeakedCopy, at(2026, 11, 17)).unwrap();
    assert_eq!(first.step, Step::Warning);
    assert_eq!(first.selling_until, None);
    assert!(r.allows(Action::Sell, at(2026, 11, 18)));
    assert_eq!(r.trust(&p, at(2026, 11, 18)).unwrap(), Trust::Reduced);

    let second = r.apply(&p, Offence::LeakedCopy, at(2027, 1, 5)).unwrap();
    assert_eq!(second.step, Step::SellingBan);
    assert_eq!(second.selling_until, Some(at(2027, 2, 5)));
    assert!(!r.allows(Action::Sell, at(2027, 1, 20)));
    assert!(r.allows(Action::Buy, at(2027, 1, 20)));
    assert!(r.allows(Action::Send, at(2027, 1, 20)));
    assert!(r.allows(Action::Chat, at(2027, 1, 20)));
    assert!(r.allows(Action::Sell, at(2027, 2, 5)));

    let third = r.apply(&p, Offence::LeakedCopy, at(2027, 3, 1)).unwrap();
    assert_eq!(third.step, Step::FullBan);
    assert_eq!(third.full_until, Some(at(2027, 4, 1)));
    let during = at(2027, 3, 15);
    for banned in [
        Action::Sell,
        Action::Buy,
        Action::Send,
        Action::Receive,
        Action::PayInPerson,
        Action::EuroDoor,
        Action::Chat,
        Action::VideoChat,
        Action::Social,
    ] {
        assert!(!r.allows(banned, during), "{banned:?}");
    }
    for open in [
        Action::View,
        Action::Object,
        Action::Reverify,
        Action::ClaimIssuance,
        Action::Vote,
    ] {
        assert!(r.allows(open, during), "{open:?}");
    }
    assert!(r.allows(Action::Send, at(2027, 4, 1)));

    // every further violation: the same full ban again
    let fourth = r.apply(&p, Offence::LeakedCopy, at(2027, 6, 1)).unwrap();
    assert_eq!((fourth.position, fourth.step), (4, Step::FullBan));
}

#[test]
fn bans_add_up() {
    let p = alpha();
    let mut r = Record::CLEAN;
    let day = at(2026, 11, 17);
    r.apply(&p, Offence::LeakedCopy, day).unwrap();
    r.apply(&p, Offence::LeakedCopy, day).unwrap();
    let third = r.apply(&p, Offence::LeakedCopy, day).unwrap();
    let fourth = r.apply(&p, Offence::LeakedCopy, day).unwrap();
    assert_eq!(third.full_until, Some(at(2026, 12, 17)));
    // the 4th ban starts when the 3rd ends
    assert_eq!(fourth.full_until, Some(at(2027, 1, 17)));
    // selling stays banned as long as the full ban runs
    assert_eq!(fourth.selling_until, Some(at(2027, 1, 17)));
    assert!(!r.allows(Action::Buy, at(2027, 1, 1)));
}

#[test]
fn two_clean_years_start_the_ladder_again() {
    let p = alpha();
    let mut r = Record::CLEAN;
    r.apply(&p, Offence::LeakedCopy, at(2026, 1, 1)).unwrap();
    r.apply(&p, Offence::LeakedCopy, at(2026, 2, 1)).unwrap();
    assert_eq!(r.trust(&p, at(2027, 12, 1)).unwrap(), Trust::Reduced);
    let later = at(2028, 3, 1);
    assert_eq!(r.position_at(&p, later).unwrap(), 0);
    assert_eq!(r.trust(&p, later).unwrap(), Trust::Ok);
    let again = r.apply(&p, Offence::LeakedCopy, later).unwrap();
    assert_eq!(again.step, Step::Warning);
}

#[test]
fn a_seller_of_a_stolen_work_skips_the_warning() {
    let p = alpha();
    let mut r = Record::CLEAN;
    let first = r.apply(&p, Offence::StolenWork, at(2026, 11, 17)).unwrap();
    assert_eq!(first.step, Step::SellingBan);
    let second = r.apply(&p, Offence::StolenWork, at(2027, 1, 1)).unwrap();
    assert_eq!(second.step, Step::FullBan);

    // a warned buyer who then sells a stolen work: the next step
    let mut r = Record::CLEAN;
    r.apply(&p, Offence::LeakedCopy, at(2026, 11, 17)).unwrap();
    let next = r.apply(&p, Offence::StolenWork, at(2026, 12, 1)).unwrap();
    assert_eq!(next.step, Step::SellingBan);
}

#[test]
fn a_clock_running_backwards_is_refused() {
    let p = alpha();
    let mut r = Record::CLEAN;
    r.apply(&p, Offence::LeakedCopy, at(2026, 11, 17)).unwrap();
    let e = r
        .apply(&p, Offence::LeakedCopy, at(2026, 11, 1))
        .unwrap_err();
    assert_eq!(e, GoodsError::Clock);
}

#[test]
fn compensation_comes_from_the_balance_only() {
    let p = alpha();
    let price = Amount::from_cents(500);
    let full = compensation(&p, price, Amount::from_cents(2_000)).unwrap();
    assert_eq!((full.paid.cents(), full.lapsed.cents()), (500, 0));
    let part = compensation(&p, price, Amount::from_cents(120)).unwrap();
    assert_eq!((part.paid.cents(), part.lapsed.cents()), (120, 380));
    let none = compensation(&p, price, Amount::ZERO).unwrap();
    assert_eq!((none.paid.cents(), none.lapsed.cents()), (0, 500));
    assert_eq!(finder_reward(&p, price).unwrap().cents(), 500);
}

#[test]
fn downloads_and_gifts() {
    let p = alpha();
    let mut l = Licence::new(account(1));
    for _ in 0..p.max_downloads {
        l.download(&p).unwrap();
    }
    assert_eq!(l.download(&p).unwrap_err(), GoodsError::DownloadsUsed);
    // gifts are off in alpha
    assert_eq!(l.give(&p, account(2)).unwrap_err(), GoodsError::GiftOff);
    let on = GoodsParams { gift: true, ..p };
    l.give(&on, account(2)).unwrap();
    assert_eq!((l.holder, l.downloads, l.generation), (account(2), 0, 1));
    assert_eq!(l.give(&on, account(2)).unwrap_err(), GoodsError::SameHolder);
    let reg = at(2026, 11, 17);
    assert_eq!(earnings_free_from(&p, reg).unwrap(), at(2026, 12, 1));
}

const BOOK: &str = "It was a bright cold day in April, and the clocks were striking \
    thirteen. The hallway smelt of boiled cabbage and old rag mats. At one end of it a \
    coloured poster, too large for indoor display, had been tacked to the wall. It \
    depicted simply an enormous face, more than a metre wide: the face of a man of \
    about forty-five, with a heavy black moustache and ruggedly handsome features. \
    He made for the stairs. It was no use trying the lift. Even at the best of times \
    it was seldom working, and at present the electric current was cut off during \
    daylight hours. It was part of the economy drive in preparation for Hate Week.";

const OTHER: &str = "Call the river by its old name and it answers slowly, carrying \
    the reeds of the north and the voices of fishermen who left long ago. The town \
    on its bank keeps a market every second morning, where bread, salt and copper \
    pans change hands under striped awnings, and children run between the stalls \
    with kites made of newspaper and string, laughing at the wind from the sea.";

#[test]
fn the_text_print_survives_small_changes() {
    let a = TextPrint::of_text(BOOK).unwrap();
    // case, punctuation, line breaks, invisible characters: no change at all
    let restyled = BOOK
        .to_uppercase()
        .replace(", ", " \u{200b}")
        .replace(". ", "\n\n");
    assert_eq!(TextPrint::of_text(&restyled).unwrap(), a);
    // a few words edited: still far above the threshold
    let edited = BOOK
        .replace("thirteen", "fourteen")
        .replace("cabbage", "potatoes");
    let s = a.similarity(&TextPrint::of_text(&edited).unwrap());
    assert!(s >= alpha().match_threshold, "{}", s.ppm());
    // a different text: near zero
    let other = a.similarity(&TextPrint::of_text(OTHER).unwrap());
    assert!(other.ppm() < 100_000, "{}", other.ppm());
    assert_eq!(a.similarity(&a), Share::ONE);
    assert!(TextPrint::of_text(" .,;! ").is_none());
    assert_eq!(word_hashes("Ünïcode wörds, 42!").len(), 3);
}

#[test]
fn the_text_print_is_fixed_for_all_time() {
    // A print stored on the chain must stay comparable: these numbers may
    // never change.
    let p = TextPrint::of_text("one two three four five six").unwrap();
    assert_eq!(p.0.len(), PRINT_LEN);
    let again = TextPrint::of_text("ONE, two; three four – five six.").unwrap();
    assert_eq!(p, again);
    assert_golden(&p);
}

#[cfg(feature = "serde")]
#[test]
fn a_text_print_travels_as_json() {
    let p = TextPrint::of_text(BOOK).unwrap();
    let json = serde_json::to_string(&p).unwrap();
    let back: TextPrint = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
    assert!(serde_json::from_str::<TextPrint>("[1,2,3]").is_err());
}

/// The first values of a known print (golden numbers).
fn assert_golden(p: &TextPrint) {
    let head: Vec<u32> = p.0.iter().take(4).copied().collect();
    assert_eq!(head, GOLDEN.to_vec(), "the print changed: {head:?}");
}

const GOLDEN: [u32; 4] = [1_802_744_596, 1_781_048_275, 600_121_677, 2_563_667_859];

#[test]
fn first_registration_wins() {
    let p = alpha();
    let print = |text: &str, h: u8| Fingerprint {
        hash: ContentHash([h; 32]),
        text: TextPrint::of_text(text),
    };
    let early = Work {
        hash: ContentHash([1; 32]),
        text: TextPrint::of_text(BOOK),
        seller: account(1),
        registered: at(2026, 1, 1),
    };
    let late = Work {
        registered: at(2026, 6, 1),
        seller: account(3),
        ..early
    };
    let works = [late, early];

    // another seller uploads the same text in another file: held, earliest wins
    let copy = print(&BOOK.replace("thirteen", "fourteen"), 9);
    match judge(&p, &copy, &account(2), &works) {
        Decision::Hold { earlier, why } => {
            assert_eq!(earlier.seller, account(1));
            assert!(matches!(why, Match::Similar(_)));
        }
        other => panic!("{other:?}"),
    }
    // the very same file
    let same = print(BOOK, 1);
    assert!(matches!(
        judge(&p, &same, &account(2), &works),
        Decision::Hold {
            why: Match::SameFile,
            ..
        }
    ));
    // the first seller's own file again, and a new edition of their own
    // work: no conflict with themselves (the later seller still conflicts)
    let solo = [early];
    assert!(matches!(
        judge(&p, &same, &account(1), &solo),
        Decision::AlreadyYours(_)
    ));
    assert_eq!(judge(&p, &copy, &account(1), &solo), Decision::Register);
    // a different work
    assert_eq!(
        judge(&p, &print(OTHER, 7), &account(2), &works),
        Decision::Register
    );
}
