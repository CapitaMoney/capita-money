# Rule check: 100, 1 000 and 100 000 members

Checks the rules of [rule-set.md](rule-set.md) (rules diary v0.25; the check is unchanged by v0.25) with numbers. Produced by [`tools/rule-check/simulate.py`](../tools/rule-check/simulate.py): expected values, monthly steps, 5 years, no randomness. Every number that is not a rule is a named assumption in the script (most are guesses, marked there); the tables at the end are its unedited output. Run `python3 tools/rule-check/simulate.py` after changing a rule or an assumption. Rule numbers below are those of the rules diary, not of rule-set.md.

## The main result

The Capita rules are linear: every member receives the same by age, whatever the size. So the money **per member** is the same at 100, 1 000 and 100 000 members (table A). What changes with size is everything around it: how many people sell, the euro costs, the work for the board, the day-one rush. Nothing in the rules breaks at 100 000 – but four things strain, and they strain at every size.

## What holds

- **Supply follows people, not trade.** A world-like member receives about 33 Capita on joining, about 96 in the first year and then about 2 × age a year while verified. After 5 years a member holds about 280 Capita, if nothing is spent (table A).
- **Lapsed members stop the flow by themselves.** Generation stops when a yearly verification is missed, so money only grows with active members. Whether 50 % or 95 % verify again changes the 5-year stock from 174 to 363 per joiner – the rule works as intended.
- **The age factor 2 stays under the work-willingness lid.** At 64, 2 × age is about 49 % of the median income in Burundi, below the 53 % lid (rules diary 2discuss Y); after 64 there is no lid by design (table C).
- **Face comparison scales.** Comparing a new face only with accounts of the same birth date means about 3 comparisons per person at 100 000 members, 171 000 in all – tiny. Expected false matches: about 17; identical twins: about 120 pairs to review once (table G).
- **The ledger is no bottleneck.** 100 000 members at half a payment a day = 40 000 transactions a day, about 0.5 per second. Four small servers cost about €290 a year at every size (table F).

## What strains

1. **Far more money than goods.** In year 1 the members receive about **80 times** the goods of the owner's year-1 scenario (121 500 Capita of goods at 100 000 members, 2discuss R) – at every size (table D). To keep up, an average member would have to buy about **8 Capita of digital goods every month**. With 5 % sellers there are 5 sellers among 100 members and 5 000 among 100 000: a small group simply has nothing to buy. Prices of digital goods will hardly rise (copies cost nothing), so the risk is not inflation but the feeling "my Capita buy nothing".
2. **The day-one rush.** If all join on day one, 100 000 members receive 3.3 million Capita that day; joining evenly over a year halves the peak month to 0.8 million (tables D, E). Gradual admission ("filled hose", waiting list) solves the rush, not the ratio of point 1.
3. **Euros cover about 2 % of all Capita.** With 10 % of members buying 20 € once and 5 % buying again yearly, euros stand behind 1.5–1.9 % of all Capita after 5 years (table A): €528 000 against 27.8 million Capita at 100 000 members (table D). Paying merchants in euros for Capita (rule 3 change, 2discuss Z) can therefore cover only about 1 Capita in 50; a pot paid out "first come, first served" runs dry.
4. **The 1 € price is about 30 times the real cost.** The own check costs almost nothing; only Didit backups cost money (about $0.33, 10 % of checks). Average real cost: about 3 US cents a check. Didit's free credit covers all backups up to about 4 500 members; at 100 000 members backups cost about $2 500 a year – but 80 000 checks at 1 € each would ask sponsors for €80 000 a year (table F).
5. **Board work grows linearly.** About 0.6 cases a year at 100 members, 3 at 1 000, **320 at 100 000** – roughly one a day, three quarters of them stopped key moves (rule 15), plus about 140 one-off dedup reviews (table G). A volunteer board manages up to about 10 000 members.
6. **The income effect is visible only in poor countries.** At age 30 one year of birthday Capita equals 23 % of a year's median income in Burundi, 12 % in India, 0.3 % in Germany (table C, PPP-adjusted). In Germany it is a gift, not an income; an experiment on the income effect (rule 1) can only measure something among members in low-income countries.
7. **30 % of a world-like membership are children** (table A). Who holds a child's key (legal check) is therefore not a side question.

## Proposals (Claude; not decided, for the rules talk)

- **P1 – First group = a trading community, not a country:** at least 1 seller in 5 members, before opening wider (2discuss R, filled hose). Measure: Capita sold per member per month; open the next group only when it rises.
- **P2 – Euros for merchants only up to the Capita bought** (2discuss Z option b or c): the merchant keeps Capita, or receives euros only out of the pot of bought Capita, capped per merchant. Never promise euros for issued Capita.
- **P3 – Verification price = real cost** (2discuss W): own check free; sponsors pay only Didit backups. Ends the paywall and lowers the sponsor need at 100 000 members from €80 000 to about $2 500 a year.
- **P4 – Second decider from 10 000 members:** a second, independent identity service decides stopped key moves (already planned "later" in rule 15), the jury takes the goods cases (rule 27 from 1 000 accounts).
- **P5 – Measure the income effect where it can be seen:** a study group in a low-income country (Kenya, India, Burundi); elsewhere present Capita as gift and community.

## What this check does not cover

Spending behaviour (who buys what), prices, network effects and the legal questions (P04.3) are not modelled; the goods benchmark is the owner's guess. Every assumption can be replaced by a measurement from the first real group.

---

## Output of the script
### A. Money per member (does not depend on the number of members)

| age profile | mean age | children | day of joining | year 1 | year 2 | year 5 | stock after 5 y | bought share |
|---|---|---|---|---|---|---|---|---|
| early adopters (adults) | 39 | 0% | 38.9 | 112 | 66.6 | 36.6 | 321 | 1.6% |
| world population | 33 | 30% | 33.3 | 96.1 | 57.5 | 31.9 | 278 | 1.9% |
| germany | 45 | 17% | 44.5 | 128 | 75.8 | 41.3 | 364 | 1.5% |

Stock after 5 years (world profile) if this share verifies again each year: 50% -> 174 · 80% -> 278 · 95% -> 363.

Capita created per member who joined on day 1 (year 1 includes the day of joining). 'Bought share' = Capita bought with euros / all Capita after 5 years.

### B. Persons (examples)

| age at joining | at once | 1st birthday | year 1 total | 5th birthday |
|---|---|---|---|---|
| 0 | 0 | 2 | 2 | 10 |
| 5 | 5 | 12 | 17 | 20 |
| 18 | 18 | 38 | 56 | 46 |
| 30 | 30 | 62 | 92 | 70 |
| 45 | 45 | 92 | 137 | 100 |
| 64 | 64 | 130 | 194 | 138 |
| 80 | 80 | 162 | 242 | 170 |

### C. Income effect: one year of birthday Capita as share of a year’s median income

| age | Burundi | India | Kenya | Germany | United States |
|---|---|---|---|---|---|
| 10 | 7.7% | 3.8% | 4.8% | 0.1% | 0.1% |
| 30 | 23.1% | 11.5% | 14.5% | 0.3% | 0.2% |
| 50 | 38.5% | 19.1% | 24.2% | 0.5% | 0.4% |
| 70 | 53.8% | 26.8% | 33.8% | 0.7% | 0.5% |

PPP-adjusted: what 2 x age per year buys locally, compared with the median income per person (World Bank PIP).

### D. Totals by size (world profile; launch = all join on day 1)

| members | day-1 payout (launch) | year 1 | stock after 5 y | euros in the pot after 5 y | goods year 1 (owner scenario) | Capita per goods Capita, year 1 |
|---|---|---|---|---|---|---|
| 100 | 3 329 | 9 714 | 27 797 | 528 | 122 | 80 x |
| 1 000 | 33 286 | 97 142 | 277 967 | 5 278 | 1 215 | 80 x |
| 100 000 | 3 328 550 | 9 714 225 | 27 796 722 | 527 827 | 121 500 | 80 x |

### E. Steady joining (evenly over the first 12 months), world profile

| members | largest month | year 1 | year 2 | active after 5 y | stock after 5 y |
|---|---|---|---|---|---|
| 100 | 801 | 6 471 | 6 302 | 41.0 | 26 340 |
| 1 000 | 8 012 | 64 714 | 63 019 | 410 | 263 405 |
| 100 000 | 801 185 | 6 471 388 | 6 301 866 | 40 960 | 26 340 455 |

### F. Checks, costs, workload per year (year 2, steady state)

| members | checks / year | sponsor need at 1 € / check | Didit backups | Didit cost ($) | servers (€ / year) | board cases / year | sellers | chain tx / day |
|---|---|---|---|---|---|---|---|---|
| 100 | 80.0 | 80.0 | 8.00 | 0.00 | 288 | 0.64 | 5.00 | 40.0 |
| 1 000 | 800 | 800 | 80.0 | 0.00 | 288 | 3.20 | 50.0 | 400 |
| 100 000 | 80 000 | 80 000 | 8 000 | 2 521 | 288 | 320 | 5 000 | 40 000 |

Didit's free credit covers the backups up to about 4 500 members. Real cost of a check: own check ~0, so about 3.30 US cents per check on average (backups only), against the 1 € price.

Board cases = stopped key moves + goods cases (below the jury size all go to the board, above it only objections). Didit cost after the free 30 checks a month.

### G. Face dedup at joining (one-off, rule 33)

| members | same birth date, per person | comparisons in all | expected false matches | twin pairs to review |
|---|---|---|---|---|
| 100 | 0.00 | 0.17 | 0.00 | 0.12 |
| 1 000 | 0.03 | 17.1 | 0.00 | 1.20 |
| 100 000 | 3.42 | 171 114 | 17.1 | 120 |

Comparing only within the same birth date keeps the work tiny even at 100 000; identical twins are the real review load.

### Assumptions

| name | value |
|---|---|
| reverify_share | 0.8 |
| door_buyer_share | 0.1 |
| door_buy_eur | 20.0 |
| door_rebuy_share | 0.05 |
| didit_share | 0.1 |
| didit_usd | 0.33 |
| didit_free_checks_month | 30 |
| key_move_rate | 0.03 |
| key_stop_share | 0.1 |
| identical_twin_share | 0.008 |
| twin_also_joins | 0.3 |
| face_false_match_rate | 0.0001 |
| goods_cases_per_member | 0.005 |
| objection_share | 0.2 |
| seller_share | 0.05 |
| tx_per_member_day | 0.5 |
| goods_per_member_year1 | 1.215 |
| nodes | 4 |
| node_eur_month | 6.0 |
