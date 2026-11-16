#!/usr/bin/env python3
"""Rule check: what the Capita rules do at 100, 1 000 and 100 000 members.

Expected values, no randomness: the same input always gives the same tables.
Monthly steps over 5 years. Standard library only.

    python3 tools/rule-check/simulate.py            # all tables (Markdown)

The rules are the ones of docs/rule-set.md. Every number that is NOT a rule
is an assumption, named in ASSUMPTIONS below with its source or "guess".
Change one and run again; docs/rule-check.md explains the results.
"""

from __future__ import annotations

# ------------------------------------------------------------------ rules
# Values from rules/alpha.toml (decided or default).
RULES = {
    "age_factor": 2,            # Capita per year of age, on each birthday (rule 4)
    "at_verification": 1,       # x age, paid at once on the day of verification (rule 4)
    "start_grant": 1.0,         # Capita per new account, spent on its own verification (rule 5)
    "verification_price_eur": 1.00,  # 1 euro = 1 verification (rule 6)
    "door_capita_per_euro": 1.00,    # euro door (rule 3)
    "door_max_payment_eur": 100.00,  # euro door, per payment (default)
    "jury_min_accounts": 1000,  # disputes go to a jury from this size on (rule 27)
}

# ------------------------------------------------------------ assumptions
ASSUMPTIONS = {
    # yearly re-verification (rule 9): share that verifies again each year. guess
    "reverify_share": 0.80,
    # euro door: share of new members who buy Capita once, and the amount. guess
    "door_buyer_share": 0.10,
    "door_buy_eur": 20.0,
    # share of verified members buying again each later year. guess
    "door_rebuy_share": 0.05,
    # own check fails -> Didit backup (rule 26). share guess; price Stand 2026-11-15
    # (didit.me pricing: ~$0.33 per full check, $10 free credit per month)
    "didit_share": 0.10,
    "didit_usd": 0.33,
    "didit_free_checks_month": 30,
    # lost key (rule 15): share of members moving to a new key per year, and
    # share of moves the old key stops (each stop = 1 board case). guess
    "key_move_rate": 0.03,
    "key_stop_share": 0.10,
    # face dedup (rule 33): identical twins among people (~4 per 1 000 births,
    # each pair = 2 people -> ~0.8 %); share whose twin also joins. guess
    "identical_twin_share": 0.008,
    "twin_also_joins": 0.30,
    # false match rate of the face check per comparison at the chosen
    # threshold. guess, to be measured (threshold 0.363 unmeasured, PR 5)
    "face_false_match_rate": 1e-4,
    # sold goods (rule 27): cases per member per year (report, warning, ban),
    # and share of bans objected to. guess
    "goods_cases_per_member": 0.005,
    "objection_share": 0.20,
    # market: share of members who sell something. guess
    "seller_share": 0.05,
    # chain: transactions per active member per day. guess
    "tx_per_member_day": 0.5,
    # owner's year-1 goods scenario (rules.md 2discuss R):
    # 121 500 Capita of goods sold at 100 000 members = 1.215 per member
    "goods_per_member_year1": 121_500 / 100_000,
    # always-on servers before public beta (rule 31: >= 4 nodes); price per
    # node and month in euros. Vermutung (Claude, ungeprüft) rules.md H: 4-6
    "nodes": 4,
    "node_eur_month": 6.0,
}

# ---------------------------------------------------------- age profiles
# Share of people per age band (lo, hi inclusive, percent). Members are
# assumed to have the age mix of the profile. Approximate figures:
#   world   = UN World Population Prospects 2024, rounded (Vermutung, ungeprüft)
#   germany = Destatis 2023, rounded (Vermutung, ungeprüft)
#   early   = guess: online early adopters, adults only, most 25-44
PROFILES = {
    "early adopters (adults)": [
        (18, 24, 15), (25, 34, 30), (35, 44, 25), (45, 54, 15), (55, 64, 10), (65, 80, 5),
    ],
    "world population": [
        (0, 4, 8.2), (5, 9, 8.3), (10, 14, 8.3), (15, 19, 8.0), (20, 24, 7.6),
        (25, 29, 7.5), (30, 34, 7.6), (35, 39, 7.1), (40, 44, 6.5), (45, 49, 6.0),
        (50, 54, 5.6), (55, 59, 5.0), (60, 64, 4.2), (65, 69, 3.5), (70, 74, 2.7),
        (75, 79, 1.8), (80, 95, 2.1),
    ],
    "germany": [
        (0, 4, 4.8), (5, 9, 4.7), (10, 14, 4.6), (15, 19, 4.7), (20, 24, 5.3),
        (25, 29, 5.6), (30, 34, 6.6), (35, 39, 6.6), (40, 44, 6.3), (45, 49, 5.9),
        (50, 54, 6.9), (55, 59, 8.2), (60, 64, 7.5), (65, 69, 6.3), (70, 74, 4.9),
        (75, 79, 4.4), (80, 95, 7.1),
    ],
}

# Median income per person and month (PPP dollars) and what 1 Capita (= 1 $
# guide value) is worth there in PPP dollars. Source:
# P04.d research table median-income-by-country.csv (World Bank PIP medians,
# ICP price levels; columns median_month_ppp and capita10_ppp / 10).
COUNTRIES = {
    "Burundi": (61.1, 2.82),
    "India": (180.8, 4.15),
    "Kenya": (97.9, 2.84),
    "Germany": (1953.4, 1.19),
    "United States": (2193.5, 0.96),
}

MEMBERS = (100, 1_000, 100_000)
MONTHS = 60


def mean_age(profile) -> float:
    total = sum(p for _, _, p in profile)
    return sum((lo + hi) / 2 * p for lo, hi, p in profile) / total


def child_share(profile) -> float:
    total = sum(p for _, _, p in profile)
    kids = 0.0
    for lo, hi, p in profile:
        years = hi - lo + 1
        kids += p * max(0, min(hi, 17) - lo + 1) / years
    return kids / total


def run(n: int, age: float, join: str) -> list[dict]:
    """Expected values per month. join = 'launch' (all on day 1) or
    'steady' (evenly over the first 12 months)."""
    a = ASSUMPTIONS
    r = RULES
    cohorts = {0: float(n)} if join == "launch" else {m: n / 12 for m in range(12)}
    rows = []
    stock = generated = bought = 0.0
    for m in range(MONTHS):
        issued = grant = checks = buy = active = 0.0
        for j, c in cohorts.items():
            t = m - j
            if t < 0:
                continue
            year = t // 12                       # 0 in the first year after joining
            verified = c * a["reverify_share"] ** year
            active += verified
            if t == 0:
                issued += c * r["at_verification"] * age
                grant += c * r["start_grant"]
                checks += c
                buy += c * a["door_buyer_share"] * min(a["door_buy_eur"], r["door_max_payment_eur"])
            else:
                # birthday falls in this month with chance 1/12; k-th birthday
                # the k-th birthday pays only if the member is still verified (rule 14)
                k = (t - 1) // 12 + 1
                still = c * a["reverify_share"] ** (k - 1)
                issued += still * r["age_factor"] * (age + k) / 12
                buy += verified * a["door_rebuy_share"] * a["door_buy_eur"] / 12
                if t % 12 == 0:
                    checks += c * a["reverify_share"] ** (t // 12)
        generated += issued + grant
        bought += buy * r["door_capita_per_euro"]
        stock = generated + bought
        rows.append(dict(month=m, issued=issued, grant=grant, bought=buy, checks=checks,
                         active=active, stock=stock, generated=generated, euros=bought))
    return rows


def year_sum(rows, key, y):
    return sum(x[key] for x in rows[12 * y:12 * (y + 1)])


def fmt(x: float) -> str:
    if abs(x) >= 100:
        return f"{x:,.0f}".replace(",", " ")
    if abs(x) >= 10:
        return f"{x:.1f}"
    return f"{x:.2f}"


def table(head, rows):
    out = ["| " + " | ".join(head) + " |", "|" + "---|" * len(head)]
    out += ["| " + " | ".join(str(c) for c in row) + " |" for row in rows]
    return "\n".join(out)


def main():
    a = ASSUMPTIONS
    print("## A. Money per member (does not depend on the number of members)\n")
    head = ["age profile", "mean age", "children", "day of joining", "year 1", "year 2",
            "year 5", "stock after 5 y", "bought share"]
    body = []
    for name, prof in PROFILES.items():
        age = mean_age(prof)
        rows = run(1, age, "launch")
        body.append([name, f"{age:.0f}", f"{child_share(prof):.0%}",
                     fmt(rows[0]["issued"]), fmt(year_sum(rows, "issued", 0)),
                     fmt(year_sum(rows, "issued", 1)), fmt(year_sum(rows, "issued", 4)),
                     fmt(rows[-1]["stock"]), f"{rows[-1]['euros'] / rows[-1]['stock']:.1%}"])
    print(table(head, body))
    world = mean_age(PROFILES["world population"])
    keep = a["reverify_share"]
    sens = []
    for share in (0.5, 0.8, 0.95):
        a["reverify_share"] = share
        rows = run(1, world, "launch")
        sens.append(f"{share:.0%} -> {fmt(rows[-1]['stock'])}")
    a["reverify_share"] = keep
    print("\nStock after 5 years (world profile) if this share verifies again each year: "
          + " · ".join(sens) + ".")
    print("\nCapita created per member who joined on day 1 (year 1 includes the day of"
          " joining). 'Bought share' = Capita bought with euros / all Capita after 5 years.\n")

    print("## B. Persons (examples)\n")
    body = []
    for age in (0, 5, 18, 30, 45, 64, 80):
        y1 = RULES["at_verification"] * age + RULES["age_factor"] * (age + 1)
        body.append([age, age * RULES["at_verification"], RULES["age_factor"] * (age + 1), y1,
                     RULES["age_factor"] * (age + 5)])
    print(table(["age at joining", "at once", "1st birthday", "year 1 total", "5th birthday"], body))

    print("\n## C. Income effect: one year of birthday Capita as share of a year’s median income\n")
    head = ["age"] + list(COUNTRIES)
    body = []
    for age in (10, 30, 50, 70):
        per_month = RULES["age_factor"] * age / 12
        body.append([age] + [f"{per_month * ppp / med:.1%}" for med, ppp in COUNTRIES.values()])
    print(table(head, body))
    print("\nPPP-adjusted: what 2 x age per year buys locally, compared with the median"
          " income per person (World Bank PIP).\n")

    print("## D. Totals by size (world profile; launch = all join on day 1)\n")
    age = mean_age(PROFILES["world population"])
    head = ["members", "day-1 payout (launch)", "year 1", "stock after 5 y", "euros in the pot after 5 y",
            "goods year 1 (owner scenario)", "Capita per goods Capita, year 1"]
    body = []
    for n in MEMBERS:
        launch = run(n, age, "launch")
        y1 = year_sum(launch, "issued", 0) + year_sum(launch, "grant", 0)
        goods = n * a["goods_per_member_year1"]
        body.append([fmt(n), fmt(launch[0]["issued"]), fmt(y1), fmt(launch[-1]["stock"]),
                     fmt(launch[-1]["euros"]), fmt(goods), f"{y1 / goods:.0f} x"])
    print(table(head, body))

    print("\n## E. Steady joining (evenly over the first 12 months), world profile\n")
    head = ["members", "largest month", "year 1", "year 2", "active after 5 y", "stock after 5 y"]
    body = []
    for n in MEMBERS:
        rows = run(n, age, "steady")
        body.append([fmt(n), fmt(max(x["issued"] for x in rows)), fmt(year_sum(rows, "issued", 0)),
                     fmt(year_sum(rows, "issued", 1)), fmt(rows[-1]["active"]), fmt(rows[-1]["stock"])])
    print(table(head, body))

    print("\n## F. Checks, costs, workload per year (year 2, steady state)\n")
    head = ["members", "checks / year", "sponsor need at 1 € / check", "Didit backups", "Didit cost ($)",
            "servers (€ / year)", "board cases / year", "sellers", "chain tx / day"]
    body = []
    for n in MEMBERS:
        rows = run(n, age, "launch")
        checks = year_sum(rows, "checks", 1) or n * a["reverify_share"]
        didit = checks * a["didit_share"]
        free = a["didit_free_checks_month"] * 12
        cost = max(0.0, didit - free) * a["didit_usd"]
        active = rows[23]["active"]
        stops = active * a["key_move_rate"] * a["key_stop_share"]
        goods = active * a["goods_cases_per_member"]
        board = stops + (goods if n < RULES["jury_min_accounts"] else goods * a["objection_share"])
        servers = a["nodes"] * a["node_eur_month"] * 12
        body.append([fmt(n), fmt(checks), fmt(checks * RULES["verification_price_eur"]), fmt(didit),
                     fmt(cost), fmt(servers), fmt(board), fmt(n * a["seller_share"]),
                     fmt(active * a["tx_per_member_day"])])
    print(table(head, body))
    free_until = a["didit_free_checks_month"] * 12 / (a["didit_share"] * a["reverify_share"])
    print(f"\nDidit's free credit covers the backups up to about {fmt(free_until)} members."
          f" Real cost of a check: own check ~0, so about {fmt(100 * a['didit_share'] * a['didit_usd'])} US cents"
          " per check on average (backups only), against the 1 € price.")
    print("\nBoard cases = stopped key moves + goods cases (below the jury size all go to the board,"
          " above it only objections). Didit cost after the free 30 checks a month.\n")

    print("## G. Face dedup at joining (one-off, rule 33)\n")
    head = ["members", "same birth date, per person", "comparisons in all", "expected false matches",
            "twin pairs to review"]
    body = []
    for n in MEMBERS:
        dates = 365.25 * 80          # birth dates in use, roughly 80 birth years
        per = max(0.0, (n - 1) / dates)
        comps = n * per / 2
        fm = comps * a["face_false_match_rate"]
        twins = n * a["identical_twin_share"] * a["twin_also_joins"] / 2
        body.append([fmt(n), fmt(per), fmt(comps), fmt(fm), fmt(twins)])
    print(table(head, body))
    print("\nComparing only within the same birth date keeps the work tiny even at 100 000;"
          " identical twins are the real review load.")

    print("\n## Assumptions\n")
    print(table(["name", "value"], [[k, v] for k, v in ASSUMPTIONS.items()]))


if __name__ == "__main__":
    main()
