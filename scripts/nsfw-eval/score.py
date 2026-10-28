#!/usr/bin/env python3
"""Scores collected NSFW classifier data offline, so thresholds can be tuned without the model.

Reads the output of collect.sh (meta.json, shots.jsonl, manifest.json, bench.json, blur/) from
one or more result directories. Directories from different models (see collect.sh --model) are
compared side by side; screenshots are matched by set name and file. For each model it replays
two scoring methods over the stored raw outputs:

- whole:   today's production path, RiskClassifier::classify_image (one whole-frame view
           behind a skin gate).
- regions: RiskClassifier::classify_regions (one view per extracted image), with every
           constant overridable.

Usage:
  score.py DIR [DIR...]                       summary for the built-in constants
  score.py DIR... --set region_score_floor=0.4 --set mod_quality=fast
  score.py DIR... --sweep region_score_floor=0.2,0.3,0.4 --sweep max_regions=1,3,6
  score.py DIR... --csv shots.csv --json scored.json --report report.html --artifact page.html
  score.py DIR... --variant "gantman:No sexy, floor 0.5:region_sexy_weight=0,region_score_floor=0.5"

--variant adds a per-image row with other settings to the report's summary table.

Parameters default to the Rust constants recorded in meta.json (skin_gate, region_skin_gate,
max_regions, region_sexy_weight, confirm_at, region_score_floor, min_agree, agree_at,
agree_share, ...), so with no --set the gantman regions method must reproduce Rust's
classify_regions; a mismatch is reported. The whole method always uses classify_image's
rules (skin_gate, no grayscale handling). Extra knobs, whose defaults are what Rust does
(or the Moderator SDK, for mod_*):

  gray_bypass        grayscale views skip the per-region skin gate (1)
  gray_skin          skin to assume for a grayscale view, where it can't be measured: a
                     number, "model" for the view's own score, or "measured" (model)
  mirror_mode        gantman: what the mirrored pass does: min, mean, off (min)
  include_whole      also score the whole frame as one more candidate region (0)
  region_skin        take the risk's skin term from the best-scoring region (1)
  max_aspect         skip regions longer than this many times their width, which the
                     model sees badly distorted; 0 keeps all (0)
  agree_in_window    measure agree_share against the window a region was found in (its
                     "container") instead of the screen: 0 never, 1 always, "desktop" on
                     desktop captures only, since phones have no windows (desktop)

min_agree, agree_at and agree_share: high risk needs min_agree regions whose score before the
floor reaches agree_at, or one such region covering agree_share of its window or screen;
otherwise the risk is held just under high. min_agree=0 turns this off.
  mod_quality        moderator crops per view: fast (1), balanced (4), accurate (8),
                     letterbox (1) (accurate)
  mod_policy         moderator heads counted: standard, allowTopless (standard)
  flag_at            the "model says NSFW" line in the summary, on the nsfw score (0.5)

Extraction thresholds (photo_colors, min_photo_fill, ...) need a re-run of collect.sh with the
matching collector flags; max_regions and min_side can only be tightened here, up to what was
collected. Labels come from labels.jsonl when present (its nsfw flag, category, kind and
size), otherwise from manifest.json, where targets starting with "normal" are safe and
everything else (porn-*, click-*) is nsfw.
"""

import argparse
import base64
import csv
import html
import itertools
import json
import sys
from pathlib import Path

GANTMAN_CLASSES = ["drawings", "hentai", "neutral", "porn", "sexy"]
HENTAI, PORN, SEXY = 1, 3, 4
MOD_HEADS = ["nipples", "genitals", "buttocks", "nude", "sexAct"]
MOD_QUALITY = {
    "fast": ["fast"],
    "letterbox": ["letterbox"],
    "balanced": ["letterbox", "z85", "z70", "z55"],
    "accurate": ["letterbox", "z85", "z70", "z55", "letterbox_m", "z85_m", "z70_m", "z55_m"],
}
MODEL_NAMES = {"gantman": "Current model", "moderator": "Moderator"}
# Risk bands, as whole percents (see HIGH_RISK_PERCENT in client/core/src/module/upload.rs
# and EXTRA_HIGH_RISK in lifecycle.rs).
MEDIUM, HIGH, EXTRA = 40, 70, 90
EXTRA_DEFAULTS = {
    "gray_bypass": 1,
    "gray_skin": "model",
    "mirror_mode": "min",
    "include_whole": 0,
    "region_skin": 1,
    "max_aspect": 0,
    "agree_in_window": "desktop",
    "mod_quality": "accurate",
    "mod_policy": "standard",
    "flag_at": 0.5,
}
# Constants older result directories don't record, set to what Rust did when they were
# collected. Their regions method is replayed with today's rules, so it won't match the Rust
# result stored with them; check_parity skips them.
LEGACY_CONSTANTS = {"region_skin_gate": 0.05, "min_agree": 0, "agree_at": 0.9, "agree_share": 0.5}
CAPTURES_PER_HOUR = 12  # one capture every 5 minutes


# ---------------------------------------------------------------------------- loading


def load(dirs):
    """Returns ({model_id: {"shots": [...], "bench": {set: bench}}}, scoring constants)."""
    models, consts = {}, None
    for d in map(Path, dirs):
        meta = json.loads((d / "meta.json").read_text())
        model = meta["model"]["id"] if isinstance(meta["model"], dict) else "gantman"
        if consts is None:
            consts = meta["scoring_constants"]
        elif consts != meta["scoring_constants"]:
            sys.exit(f"{d}: collected with different scoring constants than {dirs[0]}")
        manifest, labels = {}, {}
        if (d / "manifest.json").exists():
            manifest = {e["file"]: e for e in json.loads((d / "manifest.json").read_text())}
        if (d / "labels.jsonl").exists():
            for line in (d / "labels.jsonl").read_text().splitlines():
                e = json.loads(line)
                labels[e["file"]] = e
        entry = models.setdefault(model, {"shots": [], "bench": {}})
        if (d / "bench.json").exists():
            entry["bench"][d.name] = json.loads((d / "bench.json").read_text())
        for line in (d / "shots.jsonl").read_text().splitlines():
            rec = json.loads(line)
            m = labels.get(rec["file"]) or manifest.get(rec["file"], {})
            target = m.get("target", rec["file"].rsplit(".", 1)[0])
            nsfw = m["nsfw"] if "nsfw" in m else not target.startswith("normal")
            rec.update(
                dir=str(d), set=d.name, model=model, target=target,
                variant=m.get("size") or m.get("device") or "",
                label="nsfw" if nsfw else "safe",
                category=m.get("category") or target.split("-")[0],
                kind=m.get("kind") or "",
                device=rec.get("screen") or m.get("device")
                or ("phone" if "mobile" in d.name else "desktop"),
            )
            entry["shots"].append(rec)
    return models, consts


def parse_value(v):
    for cast in (int, float):
        try:
            return cast(v)
        except ValueError:
            pass
    return v


# ---------------------------------------------------------------------------- scoring


def gantman_nsfw(probs, sexy_weight):
    return min(max(probs[HENTAI] + probs[PORN] + probs[SEXY] * sexy_weight, 0.0), 1.0)


def moderator_nsfw(view, p):
    heads = range(5) if p["mod_policy"] == "standard" else range(1, 5)
    crops = MOD_QUALITY[p["mod_quality"]]
    return max(view["crops"][c][h] for c in crops for h in heads), len(crops)


def view_score(view, p, model):
    """A view's raw nsfw score and the model runs it took, before floor and size weight."""
    if model == "moderator":
        return moderator_nsfw(view, p)
    runs = 1
    s = gantman_nsfw(view["probs"], p["region_sexy_weight"])
    if p["mirror_mode"] != "off" and s >= p["confirm_at"]:
        runs += 1
        m = gantman_nsfw(view["mirror_probs"], p["region_sexy_weight"])
        s = min(s, m) if p["mirror_mode"] == "min" else (s + m) / 2
    return s, runs


def size_weight(region, p):
    side = min(region["width"], region["height"])
    span = p["full_weight_side"] - p["min_side"]
    t = min(max(side - p["min_side"], 0) / span, 1.0) if span > 0 else 1.0
    return p["small_region_weight"] + (1 - p["small_region_weight"]) * t


def percent(risk):
    # Rust rounds half away from zero; Python's round() is banker's rounding.
    return int(risk * 100 + 0.5)


def effective_skin(view, p, nsfw):
    """The view's skin score, or a stand-in when it's grayscale and gray_skin says so."""
    if not view.get("gray") or p["gray_skin"] == "measured":
        return view["skin"]
    if p["gray_skin"] == "model":
        return nsfw or 0.0
    return float(p["gray_skin"])


def score_whole(rec, p, model, fallback=False):
    """classify_image: whole-frame skin gate, then the model on the whole frame. With
    fallback=True, classify_regions' version for screens with no regions: the per-region skin
    gate and grayscale rules."""
    skin = rec["whole"]["skin"]
    contribution = skin * p["skin_weight"]
    gate = p["region_skin_gate"] if fallback else p["skin_gate"]
    gray = fallback and rec["whole"].get("gray")
    if contribution <= gate and not (gray and p["gray_bypass"]):
        return {"risk": contribution, "nsfw": None, "model_runs": 0}
    if model == "moderator":
        nsfw, runs = moderator_nsfw(rec["whole"], p)
    else:
        nsfw, runs = gantman_nsfw(rec["whole"]["probs"], 1.0), 1
    if fallback:
        skin = effective_skin(rec["whole"], p, nsfw)
    return {"risk": skin * p["skin_weight"] + nsfw * p["model_weight"], "nsfw": nsfw,
            "model_runs": runs}


def score_region(view, p, model, runs):
    """One crop through the per-region pipeline. Returns (score or None, runs used)."""
    gated = view["skin"] * p["skin_weight"] > p["region_skin_gate"]
    if not (gated or (p["gray_bypass"] and view["gray"])):
        return None, runs
    s, used = view_score(view, p, model)
    floor = p["region_score_floor"]
    above = max((s - floor) / (1 - floor), 0.0)
    return above * size_weight(view, p), runs + used


def score_regions(rec, p, model):
    """classify_regions, replayed from the stored outputs."""
    def keep(r):
        short, long = sorted((r["width"], r["height"]))
        return short >= p["min_side"] and (not p["max_aspect"] or long <= p["max_aspect"] * short)

    regions = [r for r in rec["regions"] if keep(r)][: int(p["max_regions"])]
    if not regions:
        out = score_whole(rec, p, model, fallback=True)
        out["region_scores"] = []
        return out
    runs, scores, agreeing, dominant = 0, [], 0, False
    screen = rec["width"] * rec["height"]
    for r in regions:
        before = runs
        s, runs = score_region(r, p, model, runs)
        scores.append(s)
        if s is not None and runs > before:
            raw, _ = view_score(r, p, model)
            if raw >= p["agree_at"]:
                agreeing += 1
                frame = screen
                in_window = p["agree_in_window"] == 1 or (
                    p["agree_in_window"] == "desktop" and rec.get("device") == "desktop")
                if in_window and r.get("container"):
                    frame = r["container"][2] * r["container"][3]
                dominant |= r["width"] * r["height"] >= p["agree_share"] * frame
    candidates = [s for s in scores if s is not None]
    if p["include_whole"]:
        whole = dict(rec["whole"], width=rec["width"], height=rec["height"])
        s, runs = score_region(whole, p, model, runs)
        if s is not None:
            candidates.append(s)
    nsfw = max(candidates) if candidates else None
    skin = effective_skin(rec["whole"], p, nsfw)
    if p["region_skin"] and any(s is not None for s in scores):
        best = max((i for i, s in enumerate(scores) if s is not None), key=lambda i: scores[i])
        skin = effective_skin(regions[best], p, scores[best])
    risk = skin * p["skin_weight"] + (nsfw or 0.0) * p["model_weight"]
    if p["min_agree"] and agreeing < p["min_agree"] and not dominant:
        risk = min(risk, (HIGH - 1) / 100)
    return {"risk": risk, "nsfw": nsfw, "model_runs": runs, "region_scores": scores,
            "agreeing": agreeing}


def cost_ms(rec, result, bench, method):
    """Estimated single-core CPU time for one capture: extraction, model build, model runs."""
    b = bench.get(rec["set"])
    if not b:
        return None
    ms = rec["timing_ms"]["extract"] if method == "regions" else 0.0
    if result["model_runs"]:
        ms += b["load_ms"] + result["model_runs"] * b["run_ms"]
    return ms


def score_model(entry, p, model):
    out = []
    for rec in entry["shots"]:
        wh, rg = score_whole(rec, p, model), score_regions(rec, p, model)
        wh["cost_ms"] = cost_ms(rec, wh, entry["bench"], "whole")
        rg["cost_ms"] = cost_ms(rec, rg, entry["bench"], "regions")
        out.append((rec, wh, rg))
    return out


def metrics(scored, method, p):
    out = {}
    idx = 1 if method == "whole" else 2
    for label in ("nsfw", "safe"):
        rows = [s[idx] for s in scored if s[0]["label"] == label]
        n = len(rows) or 1
        pct = [percent(r["risk"]) for r in rows]
        costs = [r["cost_ms"] for r in rows if r["cost_ms"] is not None]
        out[label] = {
            "n": len(rows),
            "medium+": sum(x >= MEDIUM for x in pct) / n,
            "high+": sum(x >= HIGH for x in pct) / n,
            "extra": sum(x >= EXTRA for x in pct) / n,
            "flagged": sum((r["nsfw"] or 0) >= p["flag_at"] for r in rows) / n,
            "mean_risk": sum(r["risk"] for r in rows) / n,
            "mean_runs": sum(r["model_runs"] for r in rows) / n,
            "mean_cost_ms": sum(costs) / len(costs) if costs else None,
        }
    both = [s[idx] for s in scored]
    costs = [r["cost_ms"] for r in both if r["cost_ms"] is not None]
    out["all"] = {
        "mean_runs": sum(r["model_runs"] for r in both) / max(len(both), 1),
        "mean_cost_ms": sum(costs) / len(costs) if costs else None,
        "max_cost_ms": max(costs) if costs else None,
    }
    return out


def fmt_metrics(m):
    a, b, c = m["nsfw"], m["safe"], m["all"]
    cost = f" | ~{c['mean_cost_ms']:.0f} ms/shot" if c["mean_cost_ms"] is not None else ""
    return (
        f"nsfw caught  med+ {a['medium+']:4.0%} high+ {a['high+']:4.0%} extra {a['extra']:4.0%}"
        f" | safe flagged  med+ {b['medium+']:4.0%} high+ {b['high+']:4.0%}"
        f" | runs/shot {c['mean_runs']:.1f}{cost}"
    )


def check_parity(scored, p, consts):
    """With untouched constants, the gantman replay must match Rust's classify_regions."""
    if "region_skin_gate" not in consts:
        return  # collected before today's rules; see LEGACY_CONSTANTS
    if any(p[k] != v for k, v in consts.items()) or any(
        p[k] != v for k, v in EXTRA_DEFAULTS.items()
    ):
        return
    checked = [(rec, reg) for rec, _, reg in scored if rec.get("rust")]
    if not checked:
        return
    bad = [(rec["file"], reg["risk"], rec["rust"]["risk"])
           for rec, reg in checked if abs(reg["risk"] - rec["rust"]["risk"]) > 1e-3]
    if bad:
        print(f"PARITY: {len(bad)} of {len(checked)} shots differ from Rust's classify_regions:")
        for f, py, rs in bad[:10]:
            print(f"  {f}: python {py:.4f} rust {rs:.4f}")
    else:
        print(f"parity: python replay matches Rust's classify_regions on {len(checked)} shots")


# ---------------------------------------------------------------------------- outputs


def write_csv(path, results):
    with open(path, "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["model", "set", "file", "label", "target", "variant", "width", "height",
                    "whole_skin", "whole_risk", "whole_nsfw", "whole_runs", "regions_risk",
                    "regions_nsfw", "regions", "regions_runs", "regions_cost_ms"])
        for model, scored in results.items():
            for rec, wh, rg in scored:
                f3 = lambda x: "" if x is None else f"{x:.3f}"
                w.writerow([model, rec["set"], rec["file"], rec["label"], rec["target"],
                            rec["variant"], rec["width"], rec["height"],
                            f3(rec["whole"]["skin"]), f3(wh["risk"]), f3(wh["nsfw"]),
                            wh["model_runs"], f3(rg["risk"]), f3(rg["nsfw"]),
                            len(rec["regions"]), rg["model_runs"],
                            "" if rg["cost_ms"] is None else f"{rg['cost_ms']:.0f}"])


def write_json(path, results, p):
    out = {"params": p, "models": {}}
    for model, scored in results.items():
        out["models"][model] = [
            {"set": rec["set"], "file": rec["file"], "label": rec["label"],
             "target": rec["target"], "variant": rec["variant"], "whole": wh, "regions": rg}
            for rec, wh, rg in scored
        ]
    Path(path).write_text(json.dumps(out, indent=1))


def band(risk):
    x = percent(risk)
    return "extra" if x >= EXTRA else "high" if x >= HIGH else "medium" if x >= MEDIUM else "low"


def joined(results):
    """[(rec, {model: (whole, regions)})] in the first model's order, matched by set+file."""
    models = list(results)
    index = {m: {(r["set"], r["file"]): (wh, rg) for r, wh, rg in results[m]} for m in models}
    rows = []
    for rec, _, _ in results[models[0]]:
        key = (rec["set"], rec["file"])
        if all(key in index[m] for m in models):
            rows.append((rec, {m: index[m][key] for m in models}))
    return rows


def group_table(results, key_fn, key_label):
    models = list(results)
    groups = {}
    for rec, per in joined(results):
        groups.setdefault((rec["label"], key_fn(rec)), []).append(per)
    head = "".join(
        f"<th class='num'>{MODEL_NAMES.get(m, m)}<br>whole</th>"
        f"<th class='num'>{MODEL_NAMES.get(m, m)}<br>regions</th>" for m in models
    )
    body = []
    for (label, key), rows in sorted(groups.items()):
        cells = []
        for m in models:
            for i in (0, 1):
                risks = [r[m][i]["risk"] for r in rows]
                caught = sum(percent(x) >= MEDIUM for x in risks)
                mean = sum(risks) / len(risks)
                cells.append(
                    f"<td class='num'><span class='r {band(mean)}'>{mean:.2f}</span>"
                    f"<small> {caught}/{len(risks)}</small></td>"
                )
        body.append(f"<tr><td><span class='tag {label}'>{label}</span></td>"
                    f"<td>{html.escape(key)}</td><td class='num'>{len(rows)}</td>{''.join(cells)}</tr>")
    return (f"<div class='scroll'><table><thead><tr><th>Label</th><th>{key_label}</th>"
            f"<th class='num'>n</th>{head}</tr></thead><tbody>{''.join(body)}</tbody></table></div>")


def variant_row(name, x):
    a, b, c = x["nsfw"], x["safe"], x["all"]
    cost = "–" if c["mean_cost_ms"] is None else f"{c['mean_cost_ms']:.0f}"
    return (
        f"<tr><td>{html.escape(name)}</td>"
        f"<td class='num'>{a['medium+']:.0%}</td><td class='num'>{a['high+']:.0%}</td>"
        f"<td class='num'>{a['extra']:.0%}</td><td class='num'>{a['flagged']:.0%}</td>"
        f"<td class='num'>{b['medium+']:.0%}</td><td class='num'>{b['high+']:.0%}</td>"
        f"<td class='num'>{b['flagged']:.0%}</td>"
        f"<td class='num'>{c['mean_runs']:.1f}</td><td class='num'>{cost}</td></tr>"
    )


def summary_table(results, p, variants=()):
    rows = []
    for m, scored in results.items():
        for method in ("whole", "regions"):
            x = metrics(scored, method, p)
            a, b, c = x["nsfw"], x["safe"], x["all"]
            name = f"{MODEL_NAMES.get(m, m)}, {'whole screen (today)' if method == 'whole' else 'per image'}"
            cost = "–" if c["mean_cost_ms"] is None else f"{c['mean_cost_ms']:.0f}"
            rows.append(
                f"<tr><td>{name}</td>"
                f"<td class='num'>{a['medium+']:.0%}</td><td class='num'>{a['high+']:.0%}</td>"
                f"<td class='num'>{a['extra']:.0%}</td><td class='num'>{a['flagged']:.0%}</td>"
                f"<td class='num'>{b['medium+']:.0%}</td><td class='num'>{b['high+']:.0%}</td>"
                f"<td class='num'>{b['flagged']:.0%}</td>"
                f"<td class='num'>{c['mean_runs']:.1f}</td><td class='num'>{cost}</td></tr>"
            )
    rows += [variant_row(name, x) for name, x in variants]
    n_nsfw = sum(1 for r in next(iter(results.values())) if r[0]["label"] == "nsfw")
    n_safe = sum(1 for r in next(iter(results.values())) if r[0]["label"] == "safe")
    return f"""<div class="scroll"><table>
<thead><tr><th rowspan="2">Model and method</th><th colspan="4" class="grp">NSFW screenshots caught (n={n_nsfw})</th>
<th colspan="3" class="grp">Safe screenshots flagged (n={n_safe})</th><th colspan="2" class="grp">Cost per screenshot</th></tr>
<tr><th class="num">medium+</th><th class="num">high+</th><th class="num">email</th><th class="num">model ≥ {p['flag_at']}</th>
<th class="num">medium+</th><th class="num">high+</th><th class="num">model ≥ {p['flag_at']}</th>
<th class="num">runs</th><th class="num">CPU ms</th></tr></thead>
<tbody>{''.join(rows)}</tbody></table></div>"""


def bench_table(results, models_meta):
    rows = []
    for m in results:
        b = next(iter(models_meta[m]["bench"].values()), None)
        if not b:
            continue
        size = b["model"].get("file_bytes", 0) / 1e6
        rows.append(
            f"<tr><td>{MODEL_NAMES.get(m, m)}</td><td class='num'>{size:.1f} MB</td>"
            f"<td class='num'>{b['load_ms']:.0f} ms</td><td class='num'>{b['run_ms']:.0f} ms</td>"
            f"<td class='num'>{b['rss_after_load_mb'] - b['rss_base_mb']:.0f} MB</td>"
            f"<td class='num'>{b['rss_peak_mb'] - b['rss_base_mb']:.0f} MB</td></tr>"
        )
    return (
        "<div class='scroll'><table><thead><tr><th>Model</th><th class='num'>File</th>"
        "<th class='num'>Build</th><th class='num'>One run</th><th class='num'>Memory loaded</th>"
        "<th class='num'>Memory peak</th></tr></thead><tbody>" + "".join(rows) + "</tbody></table></div>"
    )


def cost_table(results, p):
    rows = []
    for m, scored in results.items():
        for method in ("whole", "regions"):
            c = metrics(scored, method, p)["all"]
            if c["mean_cost_ms"] is None:
                continue
            hourly = c["mean_cost_ms"] * CAPTURES_PER_HOUR / 1000
            name = f"{MODEL_NAMES.get(m, m)}, {'whole screen' if method == 'whole' else 'per image'}"
            rows.append(
                f"<tr><td>{name}</td><td class='num'>{c['mean_runs']:.1f}</td>"
                f"<td class='num'>{c['mean_cost_ms']:.0f} ms</td><td class='num'>{c['max_cost_ms']:.0f} ms</td>"
                f"<td class='num'>{hourly:.1f} s</td></tr>"
            )
    return (
        "<div class='scroll'><table><thead><tr><th>Model and method</th><th class='num'>Runs per shot</th>"
        "<th class='num'>Mean CPU</th><th class='num'>Worst CPU</th><th class='num'>CPU per hour at 5 min</th>"
        "</tr></thead><tbody>" + "".join(rows) + "</tbody></table></div>"
    )


def params_table(p, consts):
    rows = "".join(
        f"<tr><td><code>{html.escape(k)}</code></td><td class='num'>{html.escape(str(v))}</td>"
        f"<td>{'changed' if consts.get(k, EXTRA_DEFAULTS.get(k)) != v else ''}</td></tr>"
        for k, v in sorted(p.items())
    )
    return ("<div class='scroll'><table><thead><tr><th>Parameter</th><th class='num'>Value</th><th></th>"
            "</tr></thead><tbody>" + rows + "</tbody></table></div>")


def region_label(view, model):
    if model == "moderator":
        crops = view["crops"]
        best = max(range(5), key=lambda h: max(v[h] for v in crops.values()))
        return f"{MOD_HEADS[best]} {max(v[best] for v in crops.values()):.2f}"
    probs = view["probs"]
    return f"porn {probs[PORN]:.2f} sexy {probs[SEXY]:.2f}"


def shot_rows(results, images):
    models = list(results)
    rows = []
    for rec, per in joined(results):
        regions = rec["regions"]
        boxes, crops = [], []
        used = len(per[models[0]][1]["region_scores"])
        for i, r in enumerate(regions):
            style = (f"left:{100 * r['x'] / rec['width']:.2f}%;top:{100 * r['y'] / rec['height']:.2f}%;"
                     f"width:{100 * r['width'] / rec['width']:.2f}%;height:{100 * r['height'] / rec['height']:.2f}%")
            boxes.append(f"<div class='box{'' if i < used else ' unused'}' style='{style}'><span>{i + 1}</span></div>")
            lines = []
            for m in models:
                scores = per[m][1]["region_scores"]
                s = scores[i] if i < len(scores) else None
                text = "not used" if i >= len(scores) else "gated" if s is None else f"{s:.2f}"
                view = next(v for v in model_records[m][(rec["set"], rec["file"])]["regions"]
                            if v["x"] == r["x"] and v["y"] == r["y"])
                lines.append(f"<div><b>{MODEL_NAMES.get(m, m)}</b> {text}"
                             f"<br><small>{region_label(view, m)}</small></div>")
            thumb = ""
            if images:
                thumb = f"<img alt='' src='{data_uri(rec['dir'], r['blur'])}'>"
            crops.append(
                f"<figure class='{'' if i < used else 'unused'}'>{thumb}"
                f"<figcaption><b>#{i + 1}</b> {r['width']}×{r['height']} · skin {r['skin']:.2f}"
                f"{' · gray' if r['gray'] else ''}{''.join(lines)}</figcaption></figure>"
            )
        frame_inner = f"<img alt='' src='{data_uri(rec['dir'], rec['blur'])}'>" if images else ""
        frame = (f"<div class='frame' style='aspect-ratio:{rec['width']}/{rec['height']}'>"
                 f"{frame_inner}{''.join(boxes)}</div>")
        risk_cells = []
        data_attrs = []
        for m in models:
            wh, rg = per[m]
            data_attrs.append(f"data-{m}-whole='{wh['risk']:.4f}' data-{m}-regions='{rg['risk']:.4f}'")
            risk_cells.append(
                f"<td class='num'><span class='r {band(wh['risk'])}'>{wh['risk']:.2f}</span>"
                f"<br><small>{wh['model_runs']} run{'s' if wh['model_runs'] != 1 else ''}</small></td>"
                f"<td class='num'><span class='r {band(rg['risk'])}'>{rg['risk']:.2f}</span>"
                f"<br><small>{rg['model_runs']} run{'s' if rg['model_runs'] != 1 else ''}</small></td>"
            )
        rows.append(
            f"<tr data-label='{rec['label']}' {' '.join(data_attrs)}>"
            f"<td><b>{html.escape(rec['target'])}</b><br>{html.escape(rec['variant'])}"
            f"<br><span class='tag {rec['label']}'>{rec['label']}</span>"
            f"<br><small>{html.escape(rec['set'])} · {rec['width']}×{rec['height']}</small></td>"
            f"{''.join(risk_cells)}<td>{frame}</td>"
            f"<td><div class='crops'>{''.join(crops) or '<small>No regions found.</small>'}</div></td></tr>"
        )
    head = "".join(
        f"<th class='num'>{MODEL_NAMES.get(m, m)}<br>whole</th><th class='num'>{MODEL_NAMES.get(m, m)}<br>regions</th>"
        for m in models
    )
    sort_options = "".join(
        f"<option value='{m}-{k}'>{MODEL_NAMES.get(m, m)}, {'per image' if k == 'regions' else 'whole screen'}</option>"
        for m in models for k in ("regions", "whole")
    )
    return head, "".join(rows), sort_options


model_records = {}


def data_uri(dir_, rel):
    data = (Path(dir_) / rel).read_bytes()
    return "data:image/webp;base64," + base64.b64encode(data).decode()


STYLE = """
/* Layout: one reading column; wide tables scroll inside their own frames. */
:root {
  --bg: #f6f7f9; --panel: #ffffff; --fg: #1b2129; --muted: #5d6875; --line: #dde2e8;
  --accent: #2f6f8f; --low: #2f7d4f; --medium: #a86400; --high: #c13a32; --extra: #7d3aa8;
  --font-body: "IBM Plex Sans", system-ui, sans-serif; --font-data: "IBM Plex Mono", ui-monospace, monospace;
}
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) {
  --bg: #12161b; --panel: #1a2028; --fg: #e4e8ee; --muted: #9aa5b2; --line: #2b333d;
  --accent: #79b4d1; --low: #6cc28d; --medium: #e0a35a; --high: #f0776d; --extra: #c79bea; color-scheme: dark; } }
:root[data-theme="dark"] {
  --bg: #12161b; --panel: #1a2028; --fg: #e4e8ee; --muted: #9aa5b2; --line: #2b333d;
  --accent: #79b4d1; --low: #6cc28d; --medium: #e0a35a; --high: #f0776d; --extra: #c79bea; color-scheme: dark; }
body { background: var(--bg); color: var(--fg); font: 15px/1.5 var(--font-body);
  padding-left: max(16px, env(safe-area-inset-left, 0px)); padding-right: max(16px, env(safe-area-inset-right, 0px));
  padding-block: 8px 48px; }
main { max-width: 1180px; margin: 0 auto; display: grid; gap: 28px; }
header { display: grid; gap: 6px; padding-top: 16px; }
h1 { font-size: 26px; line-height: 1.2; margin: 0; text-wrap: balance; letter-spacing: -0.01em; }
h2 { font-size: 18px; margin: 0 0 4px; text-wrap: balance; }
p { margin: 0; max-width: 72ch; color: var(--muted); }
p b { color: var(--fg); }
section { display: grid; gap: 10px; min-width: 0; }
.eyebrow { font: 600 12px/1 var(--font-data); text-transform: uppercase; letter-spacing: .08em; color: var(--accent); }
.scroll { overflow-x: auto; background: var(--panel); border: 1px solid var(--line); border-radius: 8px; }
table { border-collapse: collapse; width: 100%; font-size: 14px; }
th, td { padding: 8px 10px; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; }
tbody tr:last-child td { border-bottom: 0; }
th { font: 600 11px/1.3 var(--font-data); text-transform: uppercase; letter-spacing: .05em; color: var(--muted); }
th.grp { text-align: center; border-bottom: 1px solid var(--line); }
.num { text-align: right; font-family: var(--font-data); font-variant-numeric: tabular-nums; white-space: nowrap; }
small { color: var(--muted); font-size: 12px; }
code { font-family: var(--font-data); font-size: 13px; }
.tag { display: inline-block; padding: 0 6px; border-radius: 4px; font: 600 11px/1.6 var(--font-data); text-transform: uppercase; letter-spacing: .05em; }
.tag.nsfw { color: var(--high); background: color-mix(in srgb, var(--high) 14%, transparent); }
.tag.safe { color: var(--low); background: color-mix(in srgb, var(--low) 14%, transparent); }
.r { font: 600 15px var(--font-data); }
.r.low { color: var(--low); } .r.medium { color: var(--medium); } .r.high { color: var(--high); } .r.extra { color: var(--extra); }
.legend { display: flex; flex-wrap: wrap; gap: 6px 16px; font: 13px var(--font-data); color: var(--muted); }
.legend span::before { content: ""; display: inline-block; width: 9px; height: 9px; border-radius: 2px; margin-right: 6px; background: currentColor; }
.frame { position: relative; width: 260px; max-width: 100%; background: color-mix(in srgb, var(--muted) 10%, var(--panel));
  border: 1px solid var(--line); border-radius: 4px; overflow: hidden; }
.frame img { position: absolute; inset: 0; width: 100%; height: 100%; display: block; }
.box { position: absolute; border: 2px solid var(--accent); box-sizing: border-box;
  background: color-mix(in srgb, var(--accent) 12%, transparent); }
.box.unused { border-style: dashed; opacity: .55; }
.box span { position: absolute; top: 0; left: 0; background: var(--accent); color: var(--panel); font: 600 10px/1.3 var(--font-data); padding: 0 3px; }
.crops { display: flex; flex-wrap: wrap; gap: 8px; min-width: 280px; }
figure { margin: 0; width: 150px; padding: 8px; border: 1px solid var(--line); border-radius: 6px; font-size: 12px; display: grid; gap: 4px; align-content: start; }
figure.unused { opacity: .55; }
figure img { width: 100%; height: 90px; object-fit: contain; }
figcaption { display: grid; gap: 3px; }
.controls { display: flex; flex-wrap: wrap; gap: 12px; align-items: center; font-size: 14px; }
select { font: inherit; padding: 4px 6px; border-radius: 6px; border: 1px solid var(--line); background: var(--panel); color: var(--fg); }
select:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
"""

SCRIPT = """
const body = document.querySelector('#shots tbody');
const original = [...body.rows];
function apply() {
  const f = document.getElementById('filter').value, s = document.getElementById('sort').value;
  let rows = original.filter(r => f === 'all' || r.dataset.label === f);
  if (s) { const key = s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());
    rows = rows.slice().sort((a, b) => b.dataset[key] - a.dataset[key]); }
  body.replaceChildren(...rows);
}
document.getElementById('filter').addEventListener('change', apply);
document.getElementById('sort').addEventListener('change', apply);
"""


def write_page(path, results, models_meta, p, consts, images, variants=()):
    """The report. images=True inlines the blurred upload-pipeline thumbnails (local use);
    images=False draws only the region boxes on blank frames, for sharing."""
    head, rows, sort_options = shot_rows(results, images)
    shots = joined(results)
    n = len(shots)
    counts = {}
    for rec, _ in shots:
        counts[rec["set"]] = counts.get(rec["set"], 0) + 1
    sets = ", ".join(f"{k} ({v})" for k, v in counts.items())
    n_nsfw = sum(rec["label"] == "nsfw" for rec, _ in shots)
    image_note = (
        "Thumbnails are the blurred, compressed images the upload pipeline produces."
        if images else
        "This version has no screenshots. Each frame is drawn to the screenshot's shape, with the "
        "extracted images outlined; the local report has the blurred thumbnails."
    )
    fonts = ("<link rel='stylesheet' href='https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600"
             "&family=IBM+Plex+Sans:wght@400;600&display=swap'>")
    doc = f"""<title>Screenshot NSFW Evaluation</title>
<meta name="claude:viewport-fit-sides" content="cover">
{fonts}
<style>{STYLE}</style>
<main>
<header><div class="eyebrow">Virtue · classifier evaluation</div>
<h1>Screenshot NSFW evaluation</h1>
<p>{n} labeled screenshots, {n_nsfw} of them NSFW, from {html.escape(sets)}. <b>Whole screen</b> is today's production scoring: one look at the full frame behind a skin check. <b>Per image</b> finds the photos in the screenshot and scores each one. Risk is 0.3 × skin + 0.7 × model score, the same formula production uses. {image_note}</p>
<div class="legend"><span style="color:var(--low)">low &lt; 0.40</span><span style="color:var(--medium)">medium</span><span style="color:var(--high)">high ≥ 0.70</span><span style="color:var(--extra)">email ≥ 0.90</span></div>
</header>
<section><h2>Summary</h2>{summary_table(results, p, variants)}</section>
<section><h2>Model speed and memory</h2><p>Measured single-threaded on a desktop i7-9700K. Production builds the model fresh for every capture that needs it, so each capture pays the build once plus one cost per run. Phones are typically 2 to 4 times slower.</p>{bench_table(results, models_meta)}</section>
<section><h2>Cost per capture</h2><p>CPU time on that desktop for extraction, model build and model runs, averaged over these screenshots. The hourly column assumes one capture every 5 minutes, all of them like these.</p>{cost_table(results, p)}</section>
<section><h2>By category</h2><p>Borderline (swimwear, lingerie) and skin tone (beauty, fitness, medical) are safe pages chosen to tempt false positives; topless and hentai are the hardest true positives.</p>{group_table(results, lambda r: r['category'] + (' · ' + r['kind'] if r['kind'] == 'detail' else ''), 'Category')}</section>
<section><h2>By window size</h2><p>Mean risk, and how many reached medium or above.</p>{group_table(results, lambda r: r['variant'] or r['set'], 'Window')}</section>
<section><h2>By site</h2>{group_table(results, lambda r: r['target'], 'Site')}</section>
<section><h2>Screenshots</h2>
<div class="controls"><label for="filter">Show</label><select id="filter"><option value="all">All</option><option value="nsfw">NSFW only</option><option value="safe">Safe only</option></select>
<label for="sort">Sort by</label><select id="sort"><option value="">Site</option>{sort_options}</select></div>
<div class="scroll"><table id="shots"><thead><tr><th>Screenshot</th>{head}<th>Extracted images</th><th>Scores per image</th></tr></thead>
<tbody>{rows}</tbody></table></div></section>
<section><h2>Parameters</h2>{params_table(p, consts)}</section>
</main>
<script>{SCRIPT}</script>"""
    if images:
        doc = "<!doctype html><html><head><meta charset='utf-8'><meta name='viewport' content='width=device-width, initial-scale=1'>" + doc + "</html>"
    Path(path).write_text(doc)


# ---------------------------------------------------------------------------- main


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dirs", nargs="+", help="result directories written by collect.sh")
    ap.add_argument("--set", action="append", default=[], metavar="NAME=VALUE")
    ap.add_argument("--sweep", action="append", default=[], metavar="NAME=V1,V2,...")
    ap.add_argument("--csv", help="write one row per model and screenshot")
    ap.add_argument("--json", help="write every score as JSON")
    ap.add_argument("--report", help="write a self-contained HTML report with blurred thumbnails")
    ap.add_argument("--artifact", help="write the same report without any images, for sharing")
    ap.add_argument("--variant", action="append", default=[], metavar="MODEL:LABEL:K=V,K=V",
                    help="add a per-image summary row for MODEL with other settings")
    args = ap.parse_args()

    models_meta, consts = load(args.dirs)
    params = {**LEGACY_CONSTANTS, **consts, **EXTRA_DEFAULTS}
    for kv in args.set:
        k, v = kv.split("=", 1)
        if k not in params:
            sys.exit(f"unknown parameter {k}; known: {', '.join(sorted(params))}")
        params[k] = parse_value(v)
    for m, entry in models_meta.items():
        model_records[m] = {(r["set"], r["file"]): r for r in entry["shots"]}

    results = {m: score_model(entry, params, m) for m, entry in models_meta.items()}
    for m, scored in results.items():
        print(f"{MODEL_NAMES.get(m, m)}: {len(scored)} screenshots")
        check_parity(scored, params, consts)
        print("  whole   ", fmt_metrics(metrics(scored, "whole", params)))
        print("  regions ", fmt_metrics(metrics(scored, "regions", params)))

    if args.sweep:
        axes = []
        for kv in args.sweep:
            k, vs = kv.split("=", 1)
            if k not in params:
                sys.exit(f"unknown parameter {k}")
            axes.append([(k, parse_value(v)) for v in vs.split(",")])
        print("\nsweep (per-image method):")
        for m, entry in models_meta.items():
            for combo in itertools.product(*axes):
                p = {**params, **dict(combo)}
                label = " ".join(f"{k}={v}" for k, v in combo)
                x = metrics(score_model(entry, p, m), "regions", p)
                print(f"  {MODEL_NAMES.get(m, m):<14} {label:<40} {fmt_metrics(x)}")

    variants = []
    for spec in args.variant:
        m, label, kvs = spec.split(":", 2)
        p = dict(params)
        for kv in kvs.split(","):
            k, v = kv.split("=", 1)
            if k not in p:
                sys.exit(f"unknown parameter {k}")
            p[k] = parse_value(v)
        x = metrics(score_model(models_meta[m], p, m), "regions", p)
        variants.append((f"{MODEL_NAMES.get(m, m)}, per image, {label}", x))
        print(f"  variant {label:<36} {fmt_metrics(x)}")

    if args.csv:
        write_csv(args.csv, results)
    if args.json:
        write_json(args.json, results, params)
    if args.report:
        write_page(args.report, results, models_meta, params, consts, images=True, variants=variants)
        print(f"report: {args.report}")
    if args.artifact:
        write_page(args.artifact, results, models_meta, params, consts, images=False, variants=variants)
        print(f"artifact page: {args.artifact}")


if __name__ == "__main__":
    main()
