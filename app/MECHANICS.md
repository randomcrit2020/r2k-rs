# Mechanics sheet

## Quick Define (the section creator)

The original's 4-step wizard (Define > Quick Define; its pages are in shots/orig-quick*.png)
and the sections it builds (r2k 0x476eb0 with 0x476710 top bars, 0x475cf0 bottom bars, 0x4764a0
strands), checked against files the original itself saved from it (shots/cases/quick-*.rsp;
`section-response --check-quick`: all three equal, 0 mismatches, heights within 0.05 mm, the
original's own rounding):
* Step 1: Title "New Cross Section", Analysis By, f'c 35, long. fy 400, transverse fy 400,
  prestressed steel (None / 1860 MPa low relaxation or stress relieved, 13 or 15 mm strands).
  Step 2: Rectangle (b, h, 250 x 600), Circular (d), T-Beam (btop, h, bweb, ttop), I-Beam
  (btop, h, bweb, ttop, tbot, bbot). Step 3: top and bottom bars (circle: outer and inner
  rings), number and designation or "Select bar by area"; defaults 1 x 20M and 3 x 25M (circle
  16 x 25M outer, none inner). Step 4: stirrup type (None, Single Leg, Open, Closed, Hoop,
  T Headed single leg, Interlocking Hoops; Closed, circle Hoop), 10M @ 250, clear cover 40,
  bottom strands (0) with Delta-Epsilon P 6.5 mm/m.
* Materials "Concrete 1" (19 mm aggregate), "Steel 1" and "Trans", both esh 7 and eu
  100 mm/m (not 15/80); strands "Low Relax" or "Relieved" (A 0.03, B 121, C 6, constants at
  0x5301d0, 0x535460), 1860 MPa.
* Bars: first row centre at cover + stirrup diameter + db/2 (25M 25.2, 20M 19.5, 10M 11.3 mm);
  floor(((b - 2 cover - 2 db_stirrup)/db + 1)/2) bars a row, rows 2 db apart, names "bot",
  "bot<k>" ("top", "top<k>"), k = layers so far + 1. Example: 4 x 30M in a 250 mm web with
  30 mm cover gives 3 bars at 56.25 and 1 at 116.05 mm, as the original.
* Circle: the original's 12-sided outline (every 15 degrees, 0.1 mm wide at the ends); bars on
  rings, the outer at d/2 - cover - db_stirrup - db/2, bar i at 2 pi i / n from the top, pairs in
  one layer, "outer", "outer<i>"; the inner ring at d/2 - cover - 2 db_stirrup - 50 - 1.5 db
  (decoded, not yet checked against a saved file).
* Strands in rows 50.8 mm apart (0x533798) from the stirrup's inner face,
  round((b - 2 cover - 2 db_stirrup)/50.8) + 1 a row, "pbot", locked-in strain Delta-Epsilon P.
* Stirrup between its legs' centrelines (cover + db/2 from each face), legs by type (2; 1 for
  single legs), file "pattern" = the type's place in the list; loading moment only (the
  original's default); member 13 h/3 long, support h/3, load at 3 h.
* Not checked against a saved file yet: the I-beam, the inner ring, the 15 mm strands.
* Column (this program's addition to the original's four shapes): a rectangle, 400 x 400 by
  default.
  * Defaults: 3 x 25M on the top face and 3 on the bottom face (corner bars included), 1 x 25M
    on each side face (8 bars), and a closed 10M tie @ 300.
  * Side bars (any shape but the circle, "side", "side<k>") are one bar on each side face per
    layer (2 a layer), spread evenly between the first top row and the first bottom row. A
    layer closer than two bar diameters to the next is refused.
  * Drawn against the stirrup's inner faces, under the corner bars.
  * Example: 600 x 600 with 5 x 30M top and bottom and 3 x 30M a side gives rows at 66.25 and
    533.75 and side layers at 183.12, 300 and 416.88 mm.
* Bar layers: each layer is given as a distance from the bottom face (z) or from the top face
  (h - z). This program's own file attribute is `fromtop="1"`; the file keeps z either way. The
  dialog shows each layer's cover to the nearer top or bottom face (to the bar's surface).
  Quick Define marks its top rows as measured from the top.
* Clear cover is to the stirrup's outside face. The file stores the stirrup's centreline,
  distbot = cover + db/2 and disttop = h - cover - db/2. The original's Quick Define file for
  cover 40 with a 10M stirrup has 45.65 / 554.35 (shots/cases/quick-default.rsp), and
  --check-quick reproduces it. The Transverse Reinforcement dialog takes the clear covers (top,
  and bottom/sides) and the bar size, keeps the covers when the diameter changes, and checks
  that every bar row lies inside the stirrup's inner faces.
* Hoops (type 4) sit on the middle of the section, radius min(half the clear height,
  b/2 - cover - db/2); interlocking hoops (type 6) are two such hoops, one against the top cover
  and one against the bottom; the bars are placed from the first set (the top hoop), as the
  original does (0x476eb0).

## Stirrup and bar drawing (0x4a32f0 and helpers)

Compared picture by picture with the original's drawings of all six types (shots/orig-st-*.png):
* every bend is two concentric arcs (both bar faces) with bend diameter
  D = max(4 db, (57.2957 db[m] + 2.5) db) (0x4a6290); hooks are the bend plus a tail
  max(6 db, 60 mm) (0x52f080, 0x52e598), drawn as a strip one bar wide (0x4a6c90);
* closed: rounded rectangle, both ends hooked at the top-left corner with tails at -45 degrees,
  the bend opened to wrap a top bar there (0x4a69a0); open: a U whose top ends hook in and cross;
  single leg: one leg just left of the middle bar (db_bar/2 + db/2) with a 135-degree hook at
  each end; T-headed: one leg with 6 db plates at both ends and the label "T-Headed" above the
  size (0x4a7310, 0x4e6d70); hoop: a circle; interlocking: two circles, each labelled;
* the stirrup's outer faces at the clear cover from the sides (cover reduced to
  (b - 2 db)/3 in narrow webs);
* bars (0x4a2b00): circles of the bar's own diameter, white (strands grey); inside a
  rectangular stirrup a row runs edge to edge between its inner faces, otherwise the bars split
  the width in equal parts (b/(n + 1)).
* Not copied: the narrow-web case where the original draws a closed stirrup as two hooked legs.
* Bar x positions (0x4a2b00 with the clear-width routine 0x4bcf30): the clear width inside the
  stirrup at the bar's height (a hoop: its chord there); bars run edge to edge across it only
  when it is over n db and over 0.3 of the section width there, otherwise they split the
  section width in n + 1 parts. So a T-beam's 4 top bars sit at -240, -80, 80, 240 mm (two in
  the stirrup corners, two out in the flange), as the original's drawing (shots/orig-tee.png).
* Rings (file "pattern 1", "nround", "index", now kept when reading and writing): the pair
  of a ring layer at +-R sin(theta) on the ring, one label for the whole ring ("16 - 20M")
  whose leader reaches the bar just above mid-height (shots/orig-circ16.png).
* Labels: the stirrup's goes in the middle of the largest free height between bar labels
  inside the stirrup (0x4e6d70); crowded bar labels are pushed apart with slanting leaders from
  a tick just off the section's edge at the bar's height; strands carry a second line
  "Δεp = ... mm/m"; a T-beam's flange width has its own dimension line above; a circle has no
  width dimension.
* Checked: `--check-quick` now 4 cases (with the 16-bar circle, 60 mm cover), 0 mismatches.

Run-time check: `section-response --check-mechanics mechanics-check.json` recomputes, in this
build, the material values for the sample beam (against the original's printout and the sheet's
constants) and runs the sample-beam loadings the original was run on, writing both with the
differences to mechanics-check.json. Last run (2026-11-21, 74 s): 6 of 6 constants equal (ft 1.960
MPa, e'c 2.100 mm/m, vci,max 3.672 MPa, compression cracking factor 0.220 / 0.050); 14 of 14 runs
within tolerance, mean difference 2.81 % (5.86 % before the original's bond rule, 5.01 % before
the tension end below; largest now: M/V 0.6 m +9.1 %, constant 100 kNm -8.8 %).

Concrete tension capacity / axial tension end (checked against the original): the material
values match (ft 1.96 MPa, Bentz tension stiffening with the original's bond profile). The
+32 % in M-N pure tension came from where the end was taken: this rebuild swept a uniform strain
to bar rupture (1140 kN of steel plus 27 kN of concrete = 1167 kN), while the original finds its
interaction ends with load-stepping runs of axial load only (0x41dc80 sets an N-only increment
and runs the sectional solver 0x415540). Under tension the load peaks as the bars yield (760 kN
of steel plus about 118 kN the cracked concrete still carries) and then falls along the yield
plateau, so a load-stepping run stops there. The tension end is now the largest load before the
first strain hardening of a mild-steel bar: 878.4 kN against the original's 886.3 kN (-0.9 %).
SR_TENSION_RUPTURE=1 gives the old sweep.

The original's crack check on concrete tension (0x46aaa0) uses both directions:
f1 <= rx + vci/tan theta and f1 <= rz + vci tan theta.
* rz, the stirrups' reserve (layer +0x10c, recomputed every iteration by 0x4a80e0):
  k [max(sum rho fmax, sum rho fy) - sum rho fs], rho = A/(s b) per stirrup set crossing the
  depth, with a spacing factor k = clamp(0.5 (3 - s/h), 0, 1): full help up to s = h, none at
  s = 3 h. **The spacing factor is now in this rebuild** (SModel::stirrup_reserve_factor;
  SR_NO_SPACING_FACTOR=1 turns it off). For the sample beam s/h = 0.42, so k = 1 and its results
  do not change. fmax (0x4ae520) is fy, fu or the stress at a local strain at the crack; which
  one applies when was not read with certainty, so the rebuild keeps fy.
* rx, the longitudinal bars' reserve (layer +0x104, set when the layers are built, 0x4a9d90,
  from a profile made by 0x441d80; decoded with Ghidra after its project cache was rebuilt):
  - fronts: where the axial strain changes sign, scanning in from each face in tension; a face
    in tension all the way through puts its front 1000 m away (0x531af8 / 0x52e660), which
    turns the result into a plain average;
  - each bar layer in a cracked zone adds A (fmax - fs) x its distance from the front x
    min(distance to the nearest front / db, 1), + 0.1 N m (0x52f048): a moment about the front;
  - fmax (0x4ae520 with 0x4ac830): the stress at the crack's local strain
    max(1, 0.014/rho) e + (esh - ey) (0x53b180; rho = A / (0.07 m x the bars' lateral spacing),
    0x4fc340, 0x5337a0, 0.1 m when none is given), never below fy, nothing once that strain
    reaches rupture; steel fields read from 0x4aea40 (+0x10 fy, +0x20 esh, +0x38 eu, +0x40 fu);
  - that moment is carried by tension over the cracked zone (0x440400, Simpson's rule):
    uniform while at most 0.6 ft (0x52f058); then from 0.6 ft at the face rising to at most
    3 ft at the front (0x52e030); then the face value rising. Between the fronts: 3 ft.
    ft is the cracking strength (material +0x18 is the tension curve's numerator in 0x4ad9c0);
  - the crack check takes f1 <= rx + vci / tan theta (no shear: f1 <= rx).
  Built in this rebuild (SModel::long_reserve, used by both shear solvers and the plain-section
  bending), but **off by default** (SR_LONG_RESERVE=1 turns it on), because against the
  original's own runs it made the match worse: pure tension 824 kN (original 886.3, without it
  878.4), M/V 1.5 m -11.1 % (was -5.1 %), M/V 4 m -12.0 % (-6.4 %), constant shear 100 kN
  -8.9 % (-0.2 %), N = -6000 kN +5.6 % (-3.7 %). With x = 1 for the sample beam's bars the
  reserve is gone at yield, so the concrete's tension is capped at the bars' reserve and pure
  tension cannot pass about 820 kN, while the original reaches 886 kN: some part of the strain
  at the crack (the arguments of 0x4ac830, or rho) is still misread.
  Second Ghidra pass (all with Ghidra's own decompiler):
  - rho for beams is not the shell formula: TSection sets it in 0x4bc470 as the bar area on
    the bar's side of the centre over 0.8 Ag (0x52f068; Ag is +0x816, per 0x4bb270). Sample:
    bottom 1500 / 144000 = 1.04 % (x = 1.34), top 400 / 144000 = 0.28 % (x = 5.0). Now used.
    The 0.07 m x spacing formula (0x4fc340) belongs to shell elements (TShell, 0x4faf50).
  - 0x46a960 also stores the reduced cracking stress ft' at layer +0xfc.
  - 0x46aaa0 in full (Bentz type 7): first a lower bound
    f1 >= min(rx / 2, 0.85 ft tan theta + rz, ts min(0.25 ft / (0.31 + 24 w/(a + 16)) / tan theta, ft'))
    (0x52e5a0 = 0.25, 0x52e5d0 = 0.85), then the cap f1 <= min(rx + v/tan theta, rz + v tan theta)
    with the shear on the crack v = min(vci,max, |rx - rz| sin theta cos theta), and last
    f1 >= ft'/(1 + 250 w) (0x4ac920), applied after the cap, cracked or not under compression.
  - The original rebuilds the rx profile when it sets the layers up for a load stage (0x4a9d90),
    from the previous stage, so within a stage the cap lags the bars' yielding. Not copied (this
    rebuild's path solver has no load stages of that kind); the most likely reason the original
    still reaches 886 kN in pure tension.
  Measured (14 runs against the original): off 2.81 % (default); SR_LONG_RESERVE=1 (cap, beam
  rho) 4.05 %, pure tension 839 kN; SR_LONG_RESERVE=full (also the lower bound and the limited
  shear on the crack) 6.23 %, web cracking at N = -6000 kN -20 %, M/V 4 m -16 %.
  Third Ghidra pass:
  - No stage lag after all: 0x414f70 calls 0x4a9d90 repeatedly inside one load stage (a secant
    loop on the stage's unknown), and each call rebuilds rx (0x441d80) from the latest state,
    so at convergence rx matches the state, as this rebuild computes it.
  - With the cap on, uniform tension dips after yield and rises again into strain hardening
    (this build: 836 kN at 5 mm/m, 822 at 12, 866 at 20, 957 at 30), so the original's 886 kN
    end is set by where its run stops, not by the cap alone.
  - The floor ft'/(1 + 250 w) (0x4ac920 = ft' / (1 + 250000 w[m])) is the last step of
    0x46aaa0, after the cap, for every cracked layer (not only under compression). Built as
    SR_FLOOR_LAST=1 (both solvers): 4.64 % (M/V 1.5 m -2.8 %, but N = -6000 kN -30 %: the
    strain-driven solver stalls at web cracking); limited to layers not under compression,
    2.89 % - no better than the default 2.81 %, so the default keeps the earlier reading. These results are also entries of mechanics.json ("measured against the original").

Read from the original program (r2k.exe, 32-bit Windows, stripped). Addresses are
virtual addresses in that program. Internally the original works in Pa and plain
strain (compressive stress and strain negative); this rebuild works in MPa, mm and
plain strain. Values marked **estimated** were looked for in the code and not found
(or not traced far enough), so they come from engineering judgement.

## Concrete set-up (0x4ac190, 0x4aefa0, 0x4af050)

| Rule | Value | Source |
|---|---|---|
| Minimum f'c | 5 MPa (f'c is clamped to at least this) | 0x4ac190, const 0x53b2c0 = -5e6 |
| Cracking strength ft (when not given) | ft = min(0.15 f'c, 1.4 (1 + 0.01 f'c)) MPa | 0x4aefa0, consts 0x52f050, 0x52f038, 0x535ca0 |
| Peak strain e0 (when not given) | e0 = f'c / Ec' * n/(n-1), Ec' = 3320 sqrt(f'c) + 6900, n = max(0.8 + f'c/17, 1.01) | 0x4af050, consts 0x53b208, 0x53b210, 0x53b1e8, 0x5301f8 |
| Initial modulus Ec | secant of the base curve at strain -0.0001 | 0x4ac190, const 0x53b2e0 |
| Cracking strain | ecr = ft / Ec | 0x4ac190 |
| Tension stiffening factor | clamped to 0..1 | 0x4ac190 |
| Effective aggregate size | a = maxagg, reduced linearly to 0 between f'c = 60 and 70 MPa, capped at 25 mm | 0x4ac000, consts 0x53b230, 0x535ca8, 0x53b228, 0x5321e0 |

## Concrete stress (0x4ad9c0)

* Tension, eps <= ecr: f = Ec eps.
* Tension, eps > ecr, model "Bentz 1999" (default type 7): f = tsfactor * ft / (1 + sqrt(k eps)).
  k is the layer's "tension stiffening factor" (layer +0x114, set by 0x4a94d0 as
  min(kx, kz) at the layer's depth; for other tension models it is the material's own value).
  The two depth profiles are built once per section and read by straight lines between 100
  sample depths:
  - kx (0x4fec60, profile "tension stiffening factor"): for each longitudinal bar layer,
    r = pi db n / Ac with Ac the concrete within 7.5 db of the bars (0x5321e8 = 7.5, cut at the
    faces); r is spread as a triangle of peak 2.5 r (0x531ef8) over +- the crack spacing at
    the bars, the triangles added; kx = 3600 / (sum + 1e-4) with r in 1/m (0x5464e0 = 3600).
    3600 x (1/r in m) = 3.6 M in mm, so at the bars M is 0.4 Ac/(sum pi db): 2.5 times
    stiffer than the textbook bond parameter, fading to none a crack spacing away.
  - kz (0x4ff1a0, profile "tension stiffening factor Z"): where stirrups run,
    3600 / (pi db n / (b s + 1e-4 m2) + 1e-4): M = b s / (pi db n) of the stirrups.
  - Elsewhere k is about 3.6e7 (no tension stiffening). Sample beam: M 136 mm at the bottom
    bars, 210 mm at the top bars, 1061 mm in the web (stirrups).
  - Crack spacing at the bars for the triangles (0x43faf0): over the nearest bar layer above
    and below, sqrt(4 d^2 + sb^2), sb the bars' lateral spacing (file "space"; 0.1 m when
    0, 0x4fc640), 5 h with no bars, at least the aggregate size kept within 2..25 mm
    (0x52f5b8, 0x5321e0). The rebuild takes sb = 100 mm (the reference files all give 0).
  Measured: the mechanics check's 7 sectional runs plus 7 others went from a mean difference
  of 5.86 % to 5.01 % (constant 100 kNm with shear raised: -14.7 % to -8.8 %). SR_OLD_BOND=1
  brings back the earlier estimate for comparison.
* Compression softening, default type 18: beta = min(1, 1/(0.8 + max(170, 3 f'c) eps1)),
  applied to the peak stress only (peak strain unchanged). consts 0x52f068 (0.8),
  0x53b200 (170), 0x52e030 (3).
* Other softening option kept: Vecchio-Collins 1986 beta = min(1, 1/(0.8 - 0.34 eps1/e0)) (case 2, 0x53b1a8),
  and "none".
* Base curve Popovics/Thorenfeldt/Collins (type 2): fp = beta f'c,
  n = max(0.8 + fp/17, 1.1), k = 1 before the peak, k = max(1, 0.67 + fp/62) after it,
  f2 = fp * n (eps/e0) / (n - 1 + (eps/e0)^(n k)). consts 0x52f068, 0x53b1e8 (17), 0x52f5e8 (1.1),
  0x53b1c8 (0.67), 0x530940 (62).
* Parabolic base curve (type 1): f2 = fp (2 r - r^2).

## Crack check (0x46aaa0)

* Crack width w = eps1 * s_theta, s_theta = 1/(|sin theta|/sx + |cos theta|/sz).
* Maximum shear on crack vci = 0.18 sqrt(f'c) / (0.31 + 24 w/(a + 16)) (mm, MPa).
  consts 0x5334d8 (0.18), 0x5334e0 (0.31), 0x530910 (24), 0x5334d0 (0.016 m).
* f1 is limited to vci tan(theta) + rho_z (fy_z - fs_z) (standard MCFT crack check).
* Crack spacing sx, sz: sz = stirrup spacing; sx = **estimated** from the gap between
  longitudinal bar layers (min 100 mm, max 0.9 h).

## Reinforcing steel (0x4aea40, type 1)

* Elastic: fs = Es eps, up to fy.
* Yield plateau from eps_y to esh.
* Strain hardening: fs = fu + (fy - fu) (eps - eu)^2 / (esh - eu)^2 for esh <= eps <= eu.
* Beyond eu the bar has fractured (stress 0).
* File defaults (from the program's own DTD text at 0x5373f8): E = 200000 MPa, fu = 600, esh = 15 mm/m, eu = 80 mm/m.

## Prestressing steel (0x4aea40, type 2)

* Ramberg-Osgood: fps = Ep eps [A + (1 - A)/(1 + (B eps)^C)^(1/C)] <= fpu; fracture past eu.
* File defaults: E = 200000, A = 0.025, B = 118, C = 10, eu = 43 mm/m.
* Locked-in prestrain "dep" (mm/m) is added to the section strain at the tendon.
* Rupture at a crack. The original (0x4aaf70 calling 0x4ae520 and 0x4ac830) checks every bar
  and tendon at the crack, where the strain is higher than the average:
  * Strain at the crack: e_loc = x e_c + dep + (esh - fy/E).
    * e_c is the average concrete strain at the bar; only it is magnified.
    * x = max(1, k 0.014 / rho), with k = 0.625 for a bar title starting with S or T (strands),
      1 otherwise (0x53b1b8, 0x53b180); x = 2 when rho = 0 (0x52e5f8).
    * rho = the bar area on the bar's side of the centroid / (0.8 Ag) (0x4bc470).
    * For a strand fy = 0.9 fpu and esh = fy/(E + 1) (0x4ab4e0), so esh - fy/E is about 0.
  * The bar ruptures when e_loc > eu and its average strain has reached fy/E. Its stress
    becomes a sentinel that the force sums skip (0x4201e0), and a flag keeps it ruptured.
  * This rebuild gives each bar the total strain at which this happens,
    max((eu - dep - (esh - ey))/x, ey - dep) + dep, and drops its force beyond it
    (SteelProps::rupture; SR_NO_CRACK_RUPTURE=1 turns it off). The stress-strain curve itself
    is unchanged. ACI 318's et search ignores it.
  * For mild steel it rarely governs (the sample beam's bottom bars: x = 1.34, e_c 50 mm/m).
    For strands with little steel it governs: in the T-beam with 4 x 13 mm strands only,
    rho = 396 / (0.8 x 232500) = 0.213 %, x = 4.11, and rupture comes at e_c = 8.88 mm/m. The
    original's last state is 8.87 mm/m.
* Measured against the original's own runs of prestressed T-beams (800/150 flange, 250 web,
  600 deep, 4 x 13 mm low-relaxation strands at 47.65 mm, dep 6.5 mm/m; the original's Quick
  Define file shots/cases/quick-tee.rsp):

  | case | original | this rebuild | difference |
  |---|---|---|---|
  | with 4 x 30M bars, moment only | 987.0 kNm | 992.9 | +0.6 % |
  | with bars, M/V = 1.5 m | 450.0 kN | 420.5 | -6.6 %, about the sample beam's -5.1 % at the same ratio |
  | strands only, moment only | 388.9 kNm | 389.6 | +0.2 %; it was +6.5 % before the rupture rule |
  | strands only, M/V = 1.5 m | 242.6 kN | 243.5 | +0.4 % |

  The strand stress at the peak, 1728.9 MPa, is the same Ramberg-Osgood value
  (shots/orig-*-tee-*.png).

## Material factors (design resistances) (Options > General, globals 0x569f00 / 08 / 10)

How the original gets factored (code design) M-V, M-N and moment-curvature curves: it has no
separate design check; it runs the same analysis with factored materials.

* Three factors: phi_c (concrete), phi_s (reinforcing steel), phi_p (prestressing steel). They
  are 1, 1, 1 at start-up (0x4992b0). The dialog's CSA button gives 0.65, 0.85, 0.90 (CSA
  A23.3), and its None button gives 1.00. Each value is clamped to 0.01..10 (0x46fa90).
* A run is factored when |phi_c + phi_s + phi_p - 3| > 0.001. The original then prints
  "Factored Values" / "FACTORED" on its pages (0x463ad0, 0x485fc0, 0x4026e0). This rebuild
  shows a red "Factored Values" line on every page.
* Concrete (0x4ad9c0): stress = phi_c x the unfactored curve, in compression and in tension
  (tension stiffening included). Its tangent (0x4ac960) and the floor across a crack,
  ft'/(1 + 250 w) (0x4ac920), are scaled by phi_c too. Strains (e'c, the cracking strain) are
  unchanged. This rebuild also scales vci by phi_c (**inferred**: the original multiplies other
  concrete quantities by phi_c at 0x406737 and 0x41e751, not decoded further).
* Steel (0x4aea40): in a factored run the stress is held at fy (elastic-perfectly plastic, no
  strain hardening), then multiplied by phi. phi_p is used for a steel with fy above 750 MPa
  (0x4ac800), phi_s otherwise. The original's material plots stay unfactored (0x4eb9a0).
* The bar reserve at a crack (f1 <= rx, the longitudinal side of the crack check, see
  SModel::long_reserve) is on in a factored run. With the steel held at fy, yielded bars have no
  reserve, so the concrete between cracks carries no tension. This rule is what makes the
  factored moment match: the original gives 258.2 kNm, this rebuild 258.1 with the rule and
  270.6 without it. Unfactored runs keep it off (see the open unknown).
* File: `<analysisoptions phic phis phip>` ("%3.2f"), written only for a factored run (beam
  writer 0x514a20, reader 0x515ec0).
* Measured with the CSA factors, against the original's own runs of the sample beam:
  * Moment-only: Mf 258.2 kNm (shots/orig-monly-csa.png); this rebuild 258.1 (moment-curvature).
  * M/V = 1.5 m: Vf 157.9 kN (shots/orig-mv15-csa.png); this rebuild 154.8 (-2.0 %).
* The original's own factored M-V Interaction stopped at every step with a message box, so it
  was not used as a reference.

## Biaxial bending (this rebuild, biaxial.rs; not in the original)

The original bends a section about one axis only. Solve > Biaxial Bending (Mx-My) adds a load
angle alpha for the moment: Mx = M cos alpha, about the horizontal axis (+ compresses the
top, the program's usual positive moment), and My = M sin alpha, about the vertical axis
(+ compresses the right side). There is no shear.
* Fibres:
  * The concrete is the 80 layers of the moment-curvature model, each cut into 24 strips
    across its width. Each fibre has its layer's bond (tension stiffening).
  * The bars sit where the drawing places them (section_page::bar_xs, the original's 0x4a2b00
    rule). They have the same steel, prestrain and rupture-at-crack strain as the one-axis
    analysis, and the concrete they displace is taken out.
* Plane sections in two directions: e = e0 - kx (z - zc) - ky (x - xc), with (xc, zc) the gross
  centroid.
* At each curvature size:
  * e0 balances the axial load. The force is not monotonic in e0, because the concrete softens
    past its peak, so the solution is the first sign change scanning down from the tension side.
  * The curvature direction beta is found by a secant iteration so the moment points along
    alpha. A doubly symmetric section gives beta = alpha. Otherwise the neutral axis turns, as
    the solver reports (a 300 x 600 beam at alpha = 30 degrees bends almost about its weak axis,
    beta = 85 degrees).
* A run stops as the moment-curvature run does: the most compressed fibre at -12 mm/m, or the
  moment below 60 % of its peak. Its peak is the capacity along alpha.
* The Mx-My diagram at an axial load is the peaks at 8 to 72 evenly spaced angles, run in
  parallel.
* ACI 318: phi comes from the net tensile strain of the most stretched bar when the most
  compressed fibre reaches ecu (3.0 mm/m), read along the run. The design diagram is phi x
  the nominal one.
* Checks (in --check-mechanics):
  * At alpha = 0 it equals the one-axis moment-curvature peak: 355.8 against 355.7 kNm at
    N = 0, and 607.6 against 607.6 kNm at N = -3000 kN.
  * A 600 x 600 column with 16 x 30M gives the same capacity at 0 and 90 degrees (1388.2
    against 1388.6 kNm at N = -2000 kN). At 45 degrees it is 0.97 of that at N = 0 and 0.88 at
    N = -4000 kN, the usual biaxial reduction with axial load.

## AASHTO LRFD resistance factors (this rebuild; not in the original)

Options > Design Factors > AASHTO LRFD uses the same strength-reduction machinery as ACI 318
(AciFactors with code 1). The analysis stays nominal and each nominal point is multiplied by φ.
* AASHTO LRFD 9th ed. 5.5.4.2: tension-controlled 0.90 for reinforced and 1.00 for prestressed
  sections (sections with prestressing steel); compression-controlled 0.75 with ties or
  spirals; shear and torsion 0.90 (normal-weight concrete). Pn,max is 0.80 / 0.85 P0
  (5.6.4.4), ecu 3.0 mm/m (5.6.2.1).
* Transition (5.5.4.2): φ = φc + (φt - φc)(εt - εcl)/(εtl - εcl). εcl = fy/Es (2 mm/m for
  strands). εtl = 5 mm/m for reinforcement up to 517 MPa (75 ksi) and for strands, rising
  linearly to 8 mm/m at 690 MPa (100 ksi). Check: εt = 4 mm/m with 60-grade bars gives 0.850
  (ACI 318 gives 0.817).
* ACI 318's rule for shear in two directions is not applied under AASHTO; that check reports
  Vu/(φv Vn) only.

## Torsion (this rebuild, torsion.rs; not in the original)

Solid sections with closed ties. Acp and pcp come from the outline; Aoh and ph from the tie's
centreline (a rectangular tie as drawn, a hoop's circle, or two interlocking hoops' union).
At is one leg. Open ties and single legs resist no torsion.
* ACI 318-19:
  * Tth = 0.083 sqrt(f'c) Acp²/pcp sqrt(1 + Nu/(0.33 Ag sqrt(f'c))) (Table 22.7.4.1(a));
    torsion may be neglected when Tu < φ Tth.
  * Tn = 2 Ao At fyt cot θ / s, with Ao = 0.85 Aoh, θ = 45° (37.5° prestressed), fyt ≤ 420 MPa.
  * Shear with torsion (22.7.7.1(a)): sqrt((Vu/bw d)² + (Tu ph/1.7 Aoh²)²) ≤ φ(Vc/bw d +
    0.66 sqrt(f'c)), with Vc = (0.17 sqrt(f'c) + Nu/6Ag) bw d.
  * The longitudinal steel torsion needs, Al = At/s ph (fyt/fy) cot²θ, is reported.
* AASHTO LRFD:
  * Tcr = 0.331 K sqrt(f'c) Acp²/pc (MPa form of 0.126 ksi), with K = sqrt(1 + fpc/(0.331
    sqrt(f'c))) ≤ 2; torsion may be neglected when Tu ≤ 0.25 φ Tcr.
  * Tn = 2 Ao At fy cot θ / s, with Ao = 0.85 Aoh, θ = 45°, fy ≤ 690 MPa.
  * Shear with torsion: Vu,eff = sqrt(Vu² + (0.9 ph Tu/2Ao)²) against φv Vn, where Vn is the
    program's own sectional shear capacity in the shear's direction (MCFT, as AASHTO's general
    method).
* Checks by hand (600 x 600 column, 10M closed ties @ 300, f'c 35, Aoh = 508.7²):
  * ACI φTn = 43.99 kNm and φTth = 19.89 kNm.
  * AASHTO φTn = 52.79 kNm and 0.25 φ Tcr = 23.79 kNm.
  * Vc = 301.9 kN at Pu = 0 and 579.9 kN at Pu = 2000 kN.

## Load cases and D/C ratios (this rebuild, demand.rs; not in the original)

Solve > Load Cases (D/C ratios). Paste a table, one case a line: an optional name, then Pu
(kN, compression +), Mx, My (kNm), Vx, Vy (kN) and Tu (kNm). Tabs, commas, semicolons or spaces
separate the values, and header lines are skipped. For each case:
* P-M: the biaxial run at N = -Pu along the moment's direction gives Mn. D/C = Mu/(φ Mn),
  with φ from εt at ecu, or 1 for nominal and CSA. Axial compression is also checked against
  φ Pn,max (or P0 when nominal). No balanced state counts as beyond capacity.
* V: the two-direction shear run along the shear's direction at N = -Pu and M/V = (the moment
  along it)/Vu. A moment the other way is run 180° round. D/C = Vu/(φv Vn).
* T and V+T: by the chosen code (ACI 318 or AASHTO LRFD). Torsion below the code's threshold
  is reported but does not govern.
* The largest D/C governs. The results can be copied back as a tab-separated table.
* Speed (pool.rs): the cases run on a pool of one worker per processor core.
  * Each case is split into its bending job and its shear job. The shear jobs, a full sectional
    shear analysis each, are handed out first, and identical ones (same direction, axial load and
    M/V) run once.
  * Inside a shear analysis, the extra meshes of the automatic re-meshing run side by side when
    cores are free. They are kept in the same order and to the same first clear failure as
    before, so the results are identical.
  * The biaxial and two-direction shear diagrams use the same pool.
  * Measured on a 32-core machine (600 x 600 column, AASHTO): 4 cases took 28.2 s before and
    19.7 s now. The time left is the one torsion-only case's finest-mesh rerun. 24 cases take
    43.9 s, against 49.2 s with the meshes one at a time; the results are identical.

## Shear in two directions, Vx and Vy (this rebuild, shear2d.rs; not in the original)

The original takes shear in one direction only: down the page, with the moment about the
horizontal axis. Solve > Shear in Two Directions (Vx-Vy) analyses shear at any angle theta
from the vertical. Vy = V cos theta is the vertical shear that goes with Mx, and Vx = V sin theta
is the sideways shear that goes with My. The moment M = (M/V) V acts at the same angle, and the
axial load is given.

The section is turned so that theta points down the page, and the program's own sectional
shear analysis (MCFT) is run on it:
* Outline: turned about the gross centroid and read back as the total cut width at 160
  heights, so any shape works.
* Bars: at their drawing positions (section_page::bar_xs), turned with the section, and
  regrouped by exact height (no rounding: rounding to 0.25 mm changed a solver-stopped
  capacity by 6 %). Steel and prestrain are kept.
* Ties: each concrete layer gets its own tie ratio from the tie sides it crosses, one leg's
  area times the vertical part of each side's direction (shear2d::tie_area_at).
  * A closed tie has 4 sides, an open one 2 legs and the bottom, a single leg 1 side.
  * At theta = 0 this is the usual 2 legs. On a square column's diagonal a layer crosses 2 sides
    at 45 degrees, 1.41 legs. The simple projection a1 (n_v cos + n_h sin) would give 2.83.
  * Hoops are the same in every direction.
* theta = 0 is the section itself, unchanged.
* The program's sections are symmetric left to right, so only 0 to 180 degrees are run and the
  rest is mirrored. Mirror runs could otherwise differ by a few percent where the shear solver
  stops ("no balanced state").
* With no moment, the capacity follows the turned section's depth x width. A 300 x 600 beam is
  weakest sideways (244 kN at 90 degrees, 315 kN at 0). A square column is stronger on its
  diagonal (604 against 514 kN), deeper by sqrt 2 with only 1.41 legs crossing.
* Demand check: (Vux, Vuy) against the capacity interpolated at the demand's direction, Vu/Vn.
* ACI 318 (when on):
  * phi_v x the diagram.
  * The rule for shear in two directions on phi_v Vn at 0/180 and 90/270 degrees: each ratio
    may be checked alone when the other is at most 0.5; otherwise their sum must be at most 1.5.
    This is drawn and checked, and its clause should be confirmed in the user's copy of the code.
  * Also drawn: the ellipse through the four principal capacities.
* Checks (in --check-mechanics):
  * At 0 degrees it equals the one-direction shear run: 192.8 kN for the sample beam at
    M/V 1.5 m.
  * The 600 x 600 column with 16 x 30M is within 1.1 % at 90 degrees of its 0-degree value
    (508.2 / 514.0 kN, shear only).

## ACI 318 strength reduction factors (this rebuild, design.rs; not in the original)

ACI 318 factors strengths, not materials. This program therefore keeps the analysis nominal
(the full MCFT response with unfactored materials, so M, V and N interact as the mechanics say)
and reduces each nominal point by its own ACI 318-19 factor, drawn in black beside the nominal
curve (Options > Design Factors > ACI 318).
* Flexure and axial load (Table 21.2.2): phi from the net tensile strain et of the extreme tension
  steel at nominal strength.
  * et is found with the extreme compression fibre at ecu = 3.0 mm/m, plane sections, the
    section's own uniaxial materials, at the point's axial load; the curvature is found by
    bisection (design::net_tensile_strain).
  * phi = 0.65 with ties (0.75 with a spiral) up to et = ety, and 0.90 from et = ety + 3 mm/m,
    linear between.
  * ety = fy/Es, and 2 mm/m for prestressing steel (21.2.2.1).
* Shear (21.2.1): phi_v = 0.75.
* Axial cap (22.4.2): phi Pn,max = phi alpha P0, with alpha 0.80 (ties) or 0.85 (spiral), and
  P0 = 0.85 f'c (Ag - Ast - Apt) + fy Ast - (fse - 0.003 Ep) Apt.
* How each diagram is reduced:
  * M-V: each envelope point becomes (phi(et) M, phi_v V). A demand (Mu, Vu) is then inside the
    design envelope exactly when (Mu / phi, Vu / phi_v) is inside the nominal one. This is ACI's
    per-action check (Mu <= phi Mn, Vu <= phi_v Vn) applied to the interacting M-V strength
    instead of separate Mn and Vn. It gives phi Mn at pure bending and phi_v Vn at pure shear.
  * M-N: scaled radially by phi(et) at each point's N, as ACI interaction diagrams are drawn,
    and cut at phi Pn,max. The nominal polygon is cut into 10 pieces a side first, so phi varies
    smoothly through the transition.
  * N-V: phi(et) on N (cut the same way) and phi_v on V.
  * Moment-curvature: phi(et) M at the run's axial load.
* Rational rather than code-prescriptive: Mn, Vn and Pn come from the sectional analysis (MCFT,
  plane sections, strain compatibility), not from ACI's simplified Vc + Vs or stress-block
  equations. The ACI factors are applied to those nominal strengths. ACI 318-19 22.2 permits
  strain compatibility for flexure and axial load. For shear, ACI's own equations stay the code
  route; the MCFT shear here is the rational nominal resistance, as CSA A23.3 and AASHTO LRFD
  use it.
* File: this program's own attributes on `<analysisoptions>` (design="ACI318", phit, phitied,
  phispiral, spiral, phiv, ecu, pmaxtied, pmaxspiral), written with phic/phis/phip = 1.00 so the
  original reads the materials as nominal.
* Checked by hand on the sample beam:
  * P0 = 6815.4 kN and phi Pn,max = 3544.0 kN, both exact.
  * et at N = 0: 18.70 mm/m, against 18.05 by the ACI stress block, which leaves out the
    compression bars (+3.6 %).
  * phi = 0.90 at N = 0 and 0.65 at N = -3000 kN.

## Section analysis (this rebuild)

* Plane sections; 80 concrete layers over the height; width varies linearly between the
  section's outline points.
* Moment-curvature: curvature stepped, axial force held, top strain solved by Newton/bisection,
  run stops when concrete passes -0.01, a bar fractures, or moment drops below 60 % of the peak.
* Shear: shear stress distribution from the change in longitudinal force between two
  sections a short distance apart (dual-section idea used by the original, "No closure on shear
  distribution" message at 0x41f370); each layer then solved with the MCFT (above material
  models); load increased at the file's M/V ratio (sectloading MM/MV, m) until a layer fails.
  The fully coupled iteration of the original is **simplified** here: the shear-stress
  distribution is not redistributed after web layers crack and stirrups yield, so capacities can
  be conservative (sample ACI beam N1-N: 351 kN here, 457 kN measured per the file's title).
* Each layer is solved for its transverse strain and shear strain (Newton, with a bisection
  fallback) so that concrete plus stirrups carry no net transverse stress and the layer carries
  its share of V; principal stresses follow principal strains. Layers with almost no shear that
  the solve cannot close use a Mohr's-circle estimate (**estimated**).
* File loads: base axial load `BN` (kN) is used as the constant axial load; moment/shear ratio is
  `MM / MV` (m). Several stirrup sets in one section are merged into one equivalent set with
  the same steel area per metre.
* Shear flow is redistributed: after each load step the section is solved again a short distance
  along the member (moment + 2 % of the peak, same V) with the MCFT, and the change of
  longitudinal force per layer gives the new shear flow (blended 50/50, up to 4 passes).
* Strain-driven solver (path.rs) - now the default for every M-V run, with the older
  load-stepping solver as a fallback (SR_OLDSOLVER=1 forces the older one). How it adapts to
  sudden load drops: the shear is raised in levels (step halved down to 0.3 % of the shear scale
  before anything else is tried, and a step whose largest layer shear strain more than doubles
  is retaken smaller); when no balanced state exists at a higher load, "further deformed"
  starting states are tried, in this order: the fastest-changing layers deformed 1.3-40 x; a
  diagonal crack opening (those layers' shear strain grown and their stirrup strain lifted to
  0.4 x the shear strain); a curvature jump alone (flexural cracking, smallest jump that
  balances); crack plus curvature; the whole stirruped web cracking; everything scaled. The
  shear shares are re-derived for each guessed state, states with a run-away layer (shear strain
  over 25 mm/m) are discarded, and of the rest the least localised is kept. If no guess works,
  the drop itself is followed (distance along the path, then the opening layer's strain as the
  control) until the load rises past where it dropped. The corrector is Levenberg-Marquardt and
  accepts a state within 8 tolerances when it cannot improve further (perturbation slope
  matrix, rounded crack-check corners). Fallback rule: when a run stops with no balanced state
  while the web is barely deformed at its peak (largest shear strain under 1 mm/m, bars under
  1.5 mm/m), it stalled at the onset of cracking rather than failing, and the older solver's
  result is used for that ratio (the reason says so).
  Sample beam, V capacity (kN) at M/V (m) - original / this solver / older solver:
  0: 290 / 302 / 293; 0.08: 320 / 344 / 351; 0.15: 352 / stalled -> 359; 0.3: 381 / 339 / 408;
  0.45: 345 / 373 / 392; 0.6: 325 / 337 / 349; 1.0: 239 / 255 / 269; 2.0: 140 / 145 / 151;
  4.0: 76.5 / 76.8 / 88.4 (the original's values above 205 kNm are read off its straight chord).
  Full diagram (24 points, about 35 s): positive peak 398 kN at 102 kNm (original 390 at 105);
  negative side follows the original except a dip near -70 to -80 kNm (145-173 kN against
  about 190-200).
* Earlier status of the strain-driven solver (kept for the record): every layer gets its shear strain and transverse strain as unknowns
  of the whole section (with "no net transverse stress" per layer), the shear shares come from a
  dual-section secant at fixed shear strain, converged each step, and the path is traced by
  pseudo-arc-length so the capacity is the true peak of V. It uses a continuous tension curve
  (`tension_c`, rounded peak at cracking, **estimated**) and smoothed crack-check limits. Status:
  it follows the elastic and cracking range exactly (pure shear: diagonal cracking at 240-242 kN,
  original about 237). Adaptive control for sudden load drops: when arc-length steps stall, the
  strain of the fastest-changing layer is prescribed (and re-chosen every step); if that stalls,
  a jump to the balanced state at the same load with the localising layers 1.5-12 x more
  deformed; back to arc-length once the load rises again. With this it passes the drop at
  diagonal cracking (pure shear: 242 -> 174 kN, reloading to 228 kN as the stirrups engage), but
  after drops it tends to follow a branch where a single layer localises (strain grows in one
  layer until its stirrup fractures) instead of the web sharing the load, so capacities come out
  far too low (for example 121 kN at M/V = 1 m against about 350 in the original). It therefore
  stays off by default; the next step is to keep the path on the shared branch (for example by
  choosing, at each drop, the continuation with the smallest localisation).
* How each M-V point is found (after a review of the solver, see below):
  - bending-dominated ratios (a x Vcap/Mcap >= 0.35) use curvature control: the curvature is
    stepped and the centroid strain and V are solved so N is met and M = a V. This crosses the
    yield plateau of the bars without stalling; the run ends at the true peak of V (V falls by
    15 %), at bar fracture, or when no balance exists even with the curvature step halved five
    times (a confirmed web failure);
  - shear-dominated ratios use load control; a stop is confirmed only if no balance exists at
    1.005, 1.01 or 1.02 x V from three different starts, and a probe may not leap onto the
    bars' strain-hardening branch or jump in deformation (same branch only);
  - the shear flow carries over from step to step (scaled to V) and is closed with up to five
    under-relaxed dual-section passes (relaxation 0.5, 0.25, 0.1);
  - each point is labelled with what ended it (flexure, longitudinal yield, strain hardening,
    web shear, web crushing, or unclear) and drawn filled when confirmed, hollow when not;
  - extra ratios are run automatically where neighbouring points fail by different mechanisms
    or lie far apart; points out of line with their neighbours are flagged, never dropped.
  Review finding: the earlier jaggedness came mostly from load-control runs stalling on the
  bars' yield plateau and from neighbouring ratios landing on different equilibrium branches
  (yield plateau vs strain hardening). On the sample beam the positive side is now smooth
  (peak 405 kN at 157 kNm; original 390 at 105; 328 at 209 vs original 320 at 205); the
  negative side still has uncertain points near -40 to -60 kNm (shown hollow).
* (older notes) Failure of one M/V run: load steps (at most 3 % of the flexural capacity in moment) until
  equilibrium cannot be found; then higher loads (x1.02 to x1.3) are tried from scratch; with no
  unloading in the material model, any higher load that balances is carried, so the run goes on.
  When one web layer cannot carry its share, 12 % of its shear is passed to the other layers
  crossed by stirrups (shear sharing); the section fails when none has reserve (**estimated**
  rule; the original's closure loop was not decompiled).
* The chart also shows, for each run, where concrete first cracks (principal tensile strain =
  cracking strain), a longitudinal bar first yields and a stirrup first yields, interpolated
  between load steps - the same four curves the original's M-V Interaction plot shows. For the
  sample beam the cracking curve matches the original's (vertical near -38 and +42 kNm, top
  229 kN at M = 0 vs the original's -37, +58, 237 kN).
* M-V points: the person picks the points per side (4-40, default 12). Ratios: pure shear first,
  then angles spread evenly from 0 to 87 degrees in a plot where moment is divided by the
  flexural capacity and shear by the pure-shear capacity (a = tan(angle) x Mcap/Vcap). The
  original runs its diagram from its own class (TMVInter, built at 0x414700 from menu command
  5087 via 0x499b50); its ratio list is reached through virtual calls and was not read, so this
  spacing is the rebuild's own (**estimated**).
* (earlier fixed list) M-V interaction: 11 moment/shear ratios (0 to 12 d) for positive and for negative moment,
  drawn as one curve from the negative to the positive flexural capacity, as the original does.
  A run ending more than 15 % below the line through its neighbours is re-run with smaller
  load steps.
* Checked against the original on the 300 x 600 sample beam (f'c 40, 3-25M bottom, 2-15M top,
  10M @ 250): original M-V strength envelope (read from its full M-V Interaction plot) passes
  about (-117, 0), (-80, 188), (0, 290), (68, 370), (105, 390 peak), (157, 345), (205, 320) and
  then a straight chord to (338, 0) / (356, 4). This rebuild: (-128, 0), (-81, 188), (0, 289),
  (74, 344), (154, 356), (207, 319), (243, 265), (297, 138), (355, 0) - within about 10 %
  except above the original's chord at high moment, where the original computed no points.
  The original also draws cracking, longitudinal-yield and transverse-yield curves (not here).
  Interaction runs execute in parallel threads (about 9 s for this beam; the runs near the
  flexural capacity are the slowest because their load steps are halved many times).
  Algorithm note: like the original, the diagram is a series of sectional analyses taken to
  failure at fixed M/V ratios; the per-section MCFT solver and its shear-flow closure are this
  rebuild's own (the original's closure loop was not decompiled). The original's
  material summary for this beam shows ft = 1.96 MPa and e'c = 2.10 mm/m, the same as here.
  The original's crack spacing rule is shown as "2 x dist + 0.1 db/rho" (not yet used here).
* How the original lays out the runs of an interaction diagram (Ghidra, on a copy of r2k.exe
  with one header byte changed - the cache of the original had become unreadable): set-up
  0x41dc80 starts the first run at one extreme (M-V: pure bending, moment increment +1, at the
  constant axial load; N-V: pure compression, axial increment -1, at the constant moment); 0x41c610
  starts the opposite extreme (-1 / +1); 0x413e60 then starts every other run k of n: its constant
  load is the centre of the diagram, the mean of the extreme results (axial (N0 + N1) x 0.5; the
  other load the mean of four extremes, x 0.25 at 0x52e5a0), applied first and held, and its
  increment points out from that centre at angle 2 pi k / n (pi at 0x52e600), scaled like an
  ellipse to the distances of the extremes from the centre and then normalised. So the runs fan
  out from the middle of the diagram, not from zero load. The diagram types differ only by a type
  id (0 base, 2 M-V at 0x414840, 3 N-V at 0x41d210).
  The N-V diagram here now does the same (SR_NV_ORIGIN=1 gives the earlier from-zero runs): sample
  beam centre -3428 kN (original (-7392.3 + 476.8) / 2 = -3458); shear at -1300 kN 534.4 kN
  (original 535.9, -0.3 %). The envelope barely changes with the layout (peak 1018 kN either way),
  so the remaining difference near the squash load (original's peak 779.3 kN at about -6200 kN)
  comes from the sectional solver under heavy compression, not from the runs; not settled. The
  original's M-V diagram uses the same centred layout (its centre about (356 - 117) / 2 = 120 kNm),
  which this rebuild's M-V does not yet do (its runs start from zero load at fixed M/V ratios).
* N-V near the squash load - 1018 kN or 779 kN? Settled: 779 kN (the original's) is the right one
  for this model, and 1018 kN came from a missing rule plus a solver stall.
  - Same loading in both programs (constant -6000 kN, shear raised; shots/orig-n6000.png): the
    original fails at 772.6 kN by web crushing after the web has cracked along very flat cracks
    (about 10-15 degrees) and the stirrups have yielded; this program stopped at 997 kN with every
    web layer exactly at cracking (f1 = 1.92 MPa, e1 = ecr) - no failure, a stall at the uncracked
    diagonal-cracking load (by hand: f1 = ft at tau = 8.0 MPa under -31 MPa, about 960 kN).
  - The original's own stage at the bend of its curve (516 kN, shots/orig-n6000-kink.png): the web
    has already cracked at mid-depth with a peak shear stress of only 4.21 MPa, where f1 is about
    0.54 MPa - far below ft = 1.96 MPa.
  - Why (Ghidra, 0x46a960, on by default - flag 0x569f58 set by 0x4b2580 for all but one concrete
    model): the cracking stress is multiplied by clamp(1.05 - f2 / fp, 0.05, 1.0) (1.05 at
    0x52f5e0, 0.05 at 0x52f040), f2 the principal compressive stress, fp the peak of the softened
    curve - concrete compressed near its strength cracks at a small fraction of ft, as biaxial
    tests (Kupfer) show. Its plot agrees: at the top of the web f2 / fp is about 0.83, factor
    0.22, cracking stress 0.43 MPa - its red line there reads 0.42. After cracking it keeps a floor
    ft' / (1 + 250 w) on the stress across a crack (0x4ac920, 250000 per m at 0x53b220).
  - This program now does both (path.rs layer_point and the load-stepping layer; the post-cracking
    branch is the same curve scaled by the factor - **estimated**, the original keeps its own and
    lets its crack check govern; SR_NO_CRACK_REDUCTION=1 turns it off, SR_CRACK_MODE=A tries the
    unscaled curve, which does not reproduce the original under compression: 997.6 kN). Result at
    -6000 kN: cracking at 504-524 kN (original's bend 516), capacity 740.8 kN (original 772.6,
    -4 %). The N-V diagram now uses constant-axial-load shear runs with both solvers (the strain-
    driven one can stall just after cracking; a stall only under-estimates, so the larger peak is
    kept): peak about 798 kN at -6423 kN (original 779.3), 769.3 kN at -6090 (original 772.6 at
    -6000). Takes about 2.5 minutes.
  - Effect elsewhere (the original's 7 runs at zero axial load): mean difference 5.0 % -> 6.3 %
    (M/V 0.6 m +12.2 -> +5.9 %; constant 100 kNm -12.6 -> -14.7 %; constant 100 kN shear -1.1 ->
    -6.5 %; M/V 4 m +0.4 -> -7.3 %, a run that is sensitive to small changes). The rule is the
    original's own, so it stays on.
* Ghidra repaired (2026-11-21): its saved project for r2k.exe could not be opened ("Unsupported
  file system schema ... versioned") because the versioned folder's index file was missing (only
  ~index.bak and ~index.tmp, an interrupted save); the index was restored from ~index.bak and the
  original opens in Ghidra again. (Deleting the cache was refused by auto mode, which only the
  person can allow.)
* M-V with the original's centred run layout (Solve > "M-V Interaction, original's run layout"):
  pure positive and negative bending first, centre moment their mean (sample beam 113.2 kNm),
  every other run holds the centre moment and loads out from it at angle pi k / (2 n), runs heading
  for negative moment on the mirrored section, each with the automatic re-meshing. Sample beam
  against the original: (-3.6, 298.9) vs (0, 290 plot / 319.3 own run); (79.4, 380.2) vs
  (68, 370); (146.2, 369.4) vs (157, 345); (206.2, 331.6) vs (205, 320); (-79, 164) vs (-80, 188);
  two runs near 45-65 kNm still stop early (241, 274 kN, hollow). The default M-V (runs from zero
  load at fixed M/V ratios, with cross-checks) is kept: overall it is as close, and steadier.
* M-N and N-V interaction (Solve menu and toolbar). How the original does them (Ghidra): one class
  TMNV_baseInter (constructor 0x41c340, set-up 0x41dc80) with TMNInter, TMVInter (0x414700) and
  TNVInter; every diagram is a series of full sectional analyses, each loading the section along
  one direction in the plane of two loads with the third held at its constant value, each taken
  to its peak. Its control plots name the pairs: "Control : Moment - Axial Load" with a family of
  moment-curvature curves (M-N: the bending capacity at a series of constant axial loads), and
  "Control : Axial Load - Shear" (N-V). This rebuild:
  - M-N: moment-curvature runs at constant axial loads, both moment directions, closed by the
    pure-compression and pure-tension capacities from a uniform-strain sweep (the moment there
    is not zero when top and bottom steel differ). Fixed on the way: the moment-curvature solver
    now finds the centroid strain on the rising branch under heavy compression (it failed above
    about 2400 kN compression), and its "moment fell well below the peak" stop no longer misfires
    when the moment starts negative.
  - N-V: radial runs with the strain-driven solver at the section's constant moment, from pure
    compression to pure tension, with two rounds of extra directions where neighbouring results
    lie far apart.
  Checked against the original run on the sample beam (shots/orig-mn.png, orig-nv.png):
  M-N peak moments 642.0 / 641.0 kNm and -652.3 / -648.2 kNm, compression end -7794.5 / -7884 kN
  (+1.1 %), tension end 886.3 / 1162 kN (+31 %: the original's limit there was not identified).
  N-V: compression end -7392.3 / -7422 kN, tension end 476.8 / 563 kN, shear at -1283 kN
  535.9 / about 512 kN (-4.5 %); but the peak shear under heavy compression is 779.3 in the
  original against 1017 here (+30 %, at about -6500 kN): this program's shear solver
  over-estimates near the squash load.
* Shear on crack (decompiled with Ghidra, 0x46aaa0, the original's crack check): the steel crossing
  the crack can add rho (fy - fs) in each direction (longitudinal: reserve_x, transverse:
  reserve_z); the direction with the smaller reserve governs - if reserve_x <= reserve_z,
  vci = (f1 - reserve_x) tan(theta), otherwise vci = (f1 - reserve_z) / tan(theta); zero when no
  steel crosses the crack (both reserves under 1 Pa). tan(theta) is only kept within 1e-6..1e6
  (0x52e570, 0x52e680). The same choice sets the crack-check limit on f1: f1 <= reserve_x +
  vci,max / tan(theta) when the longitudinal reserve is smaller (always so in web layers without
  bars), else f1 <= reserve_z + vci,max tan(theta). The nine plots' "Shear on Crack" now uses this
  (reserve_x from the bars inside each layer at their current stress), capped at vci,max for
  display; it replaces the earlier estimated definition. Finding: this program's solver applies
  only the transverse form of the limit (f1 <= vci,max tan(theta) + reserve_z), so in the web it
  lets f1 - and the shear on crack - run higher than the original's longitudinal form allows
  (sample beam at the peak: the blue curve reaches the red limit in the lower web, where the
  original's stays near 0.2 MPa). Changing the solver's crack check to the original's rule is the
  next step; it is likely part of the remaining 12 % differences.
* How the original spreads the shear over the depth (decompiled with Ghidra, 0x41e840, the
  routine that builds the "Calculated Shear stress distribution" and raises "No closure on shear
  distribution"): a dual-section step each iteration - a linear strain change between two nearby
  sections times each node's current stiffness gives the change of longitudinal stress, which is
  integrated from the top into the shear flow and divided by the width (shear stress), then
  normalised to the applied shear. Each new value is averaged 50/50 with the previous
  iteration's (0.5, 0x52e5b8), may change by at most 10 x per iteration (100 x for a material
  named "steel"), and each layer's shear stress is capped at 8 x an average one (8.0, 0x5308f8).
  This rebuild already used the dual-section change of longitudinal force blended with the
  previous shares; it now also caps each layer at 8 x its share of the concrete area
  (**estimated** average: the original's divisor was not identified). Against the original's
  own sectional runs on the sample beam: mean difference 5.9 % -> 5.0 % (constant 100 kN shear:
  -6.5 % -> -1.1 %; M/V 4 m +1.3 % -> +0.4 %; others unchanged).
* The M-V plot's design-limit curves end at zero shear on the moment-curvature curve's own limit
  moments (sample beam: steel 322.3 kNm, concrete 346.5 kNm; and the negative side's).
* Design strain limits (this program's own, a designer's choice, set per material in Define >
  Materials): concrete ecu (default 3.5 mm/m) and, per rebar, a strain limit es (default: the
  start of strain hardening esh, 15 mm/m in the original's file defaults). They are not analysis
  cut-offs: the moment-curvature view writes and marks the moments where the most compressed
  concrete fibre (longitudinal strain) reaches ecu and where a bar reaches es; the M-V view marks,
  for each run that gets there before its peak, where that happens (orange squares = concrete,
  teal = steel, joined only between neighbouring runs that both got there). Saved in the file as
  extra attributes "ecu" on concrete and "elim" on rebar, only when changed. Checked on the
  sample beam: es = 15 mm/m is reached at M = 322.3 kNm (32.2 rad/km), exactly where the
  moment-curvature curve leaves the yield plateau and starts to harden; ecu = 3.5 mm/m at
  M = 346.5 kNm (47.7 rad/km), before the peak of 354.7 kNm at 57.2 rad/km.
* Nine plots (Sectional Response view), redrawn after the original's (its window was captured
  for the sample beam, shots/orig-mv15.png): a left panel "Nine Graphs" with the current loads and
  two control plots (shear - average shear strain and moment - curvature; moment - bottom strain
  for a bending-only run), the current stage marked by dashed green lines; a 3 x 3 grid in the
  original's order - Cross Section (concrete in compression shaded darker, the stirrup with
  hooks, bars white while elastic, red when yielded in tension, blue in compression, darker in
  strain hardening), Longitudinal Strain, Transverse Strain, Crack Diagram (grey, cracks traced
  layer by layer along the compression direction, widths in mm written along one crack), Shear
  Strain, Shear Stress, Principal Compressive Stress (red: the softened strength), Shear on Crack
  (red: +- vci,max for the crack width; blue: what the crack must carry with the stirrups there at
  yield, (f1 - rho_z (fyz - fsz)) / tan theta - **estimated** definition), Principal Tensile
  Stress (red: ft before cracking, then the smaller of tension stiffening and the crack check).
  Each profile has big titles, "top"/"bot" beside the zero axis and the extreme values written on
  a line across the middle, as the original does. The view opens at the peak stage.
* M-V view: the original's comparison line is no longer drawn (the table stays, folded), the long
  explanation is a hover note, and the point counts sit in the legend column.
* Layers ("mesh") and the envelope (2026-11-21). Shear capacity (kN) on the sample beam against
  the number of layers, plain M/V runs, no re-meshing:
  | M/V (m) | 0 | 0.08 | 0.12 | 0.15 | 0.2 | 0.25 | 0.3 | 0.45 | 0.6 | 1.0 |
  | 20 layers | 312.8 | 340.0 | 349.1 | 361.2 | 376.0 | 281.6 | 326.1 | 374.0 | 335.6 | 265.8 |
  | 40 | 304.3 | 343.2 | 351.3 | 358.7 | 382.4 | 269.6 | 342.7 | 374.8 | 338.9 | 263.5 |
  | 60 | 317.3 | 344.1 | 352.4 | 330.2 | 232.5 | 272.5 | 344.9 | 374.5 | 338.5 | 258.9 |
  | 80 | 317.7 | 343.3 | 355.9 | 228.5 | 233.1 | 369.2 | 399.6 | 355.8 | 338.4 | 253.7 |
  Runs that finish normally agree within a few % at any layer count (pure shear 304-318, the
  original 319.3), so the layers are fine enough; but in the zone of maximum shear (M/V 0.15 -
  0.3) single runs stall just after diagonal cracking (228-282 kN, web shear strain 1.3-1.9
  mm/m, against 8-12 mm/m at real failures), and which ratio stalls changes with the layer
  count: the gaps were solver stops, not the mesh being too coarse. Changes:
  - automatic re-meshing: a run without unambiguous failure (fracture, crushing, 25 mm/m shear
    strain, or the load falling past its peak) is run again with 1.5 x, 0.75 x and 2 x the
    layers (stopping once one fails unambiguously); of the runs that fail for real the largest
    is kept, and the reason lists every mesh tried. "No balanced state" only counts as failure
    with a stirrup or bar at yield or 4 mm/m web shear strain. The original's 7 runs: mean
    difference 7.0 % -> 5.9 % (pure shear 304.5 -> 317.7 kN against 319.3; M/V 4 m 81.0 ->
    88.6 against 87.4; constant 100 kN shear 323.9 -> 306.9 kNm against 328.1);
  - the envelope line now goes through the outer points only: in a plot scaled by each side's
    bending capacity and the largest shear, a point more than 8 % short of the line through its
    neighbours, along its own loading direction, lies inside (its run stopped early) and is shown
    as a grey x, not joined (repeated until none is left). The first try - dropping any point with
    another carrying both more moment and more shear - was wrong for this beam, whose shear
    capacity rises with moment up to about 100 kNm (it hid 53 real points);
  - checks that stopped without failure proof are no longer drawn on the plot (only in the
    table), and the legend sits beside the plot instead of over the descending branch.
  40 points per side: about 2 minutes (re-meshed runs cost up to 4 x).
* Speed and threads: the M-V diagram runs its points in parallel (one thread per M/V ratio,
  both moment directions at once, the cross-checks and the original's 7 load cases in parallel
  too); a single run is one thread. Splitting a single run's slope matrix over threads was tried
  and made it slower (36.6 s -> 59.6 s for the 7 cases: each matrix is too small for the cost of
  starting threads), so instead the run itself was made faster. Each layer's shear and
  transverse strain act only on that layer, so the slope matrix is now built from 6 section
  evaluations instead of 83 (SR_JACFD=1 gives the old one-column-at-a-time matrix for checking),
  and the corrector's normal equations add up only each row's non-zero entries. The 7 cases:
  36 s -> 9.3 s, results the same within 1.5 % (pure shear 304.5 -> 304.3 kN, constant 100 kNm
  340.6 -> 335.7 kN); the full M-V diagram with its checks: about 31 s -> 16 s. Measured with
  SR_PROF=1: what's left is mostly the dense solve in the corrector.
* The original run directly (2026-11-21): r2k.exe was started with the sample beam saved with
  each loading below (shots/cases/*.rsp), Solve > Sectional Response was pressed through its
  menu, and the peak "Current Loads" were read off its window (pictures shots/orig-*.png).
  Original M, V at the peak / this program (Solve > Check against the original's runs):
  shear only 0, 319.3 / 304.5 kN (-4.6 %); M/V 0.6 m 181.0, 301.8 / 337.8 kN (+11.9 %);
  M/V 1.5 m 304.3, 203.1 / 182.0 kN (-10.4 %); M/V 4 m 349.3, 87.4 / 81.0 kN (-7.4 %);
  constant 100 kNm + shear 100.0, 387.9 / 340.6 kN (-12.2 %); constant 100 kN + moment 328.1 /
  323.9 kNm (-1.3 %); moment only 356.4 / 360.3 kNm (+1.1 %). Mean 7.0 %. The original's own
  sectional runs are not the same as its M-V plot: pure shear 319 in a sectional run against
  about 290 on its M-V diagram, so the plot readings used before are only a rough reference.
  Its runs also show the drop at diagonal cracking and the reload (pure shear: peak near
  285 kN, fall, rise to 319 kN at 5.8 mm/m shear strain), the same shape as here.
* Doubtful M-V points are now checked along both other loading paths the original's
  constant + increment loading allows: moment held while the shear rises, and shear held while
  the moment rises. A check counts only with real failure evidence (fracture, crushing, very large
  shear strain, the shear falling past its peak, or no balance at a higher load with the web
  clearly working, shear strain >= 1 mm/m). Agreement within 5 % with either confirms the point;
  if the M/V run had no failure proof and a check reaches further out, the point moves there;
  otherwise it stays hollow. Shown in the M-V view (triangles = moment held, diamonds = shear
  held, filled = real failure) and in a table. Sample beam: the negative side's four doubtful
  points (-59.7 to -118 kNm) agree both ways (e.g. -59.7 kNm: 204.5 / 205.0 kN / 62.6 kNm); the
  positive side between 260 and 325 kNm still disagrees - the moment-held runs carry 7-90 % more
  shear there but mostly stop without failure proof, one (323.8 kNm) moved out to 158.8 kN.
* Loading, read from the original (supersedes the next item): its file has six load values,
  BN, BM, BV (constant axial load, moment, shear) and MN, MM, MV (their increments), with
  defaults 0, 0, 0, 0, 1, 0 (DTD text at 0x538f59: a moment-only increment by default). Its
  results report N_u = BN + f MN, M_u = BM + f MM, V_u = BV + f MV with f the load factor
  (0x406470: "N_u = %5.0f", "(local_80 * f + local_68)" etc.), it warns "Negative Load Factor!
  Lower initial loads" when the constant loads alone can't be carried, and "Unable to converge on
  dead load solution" (0x432550), so the constant ("dead") state is solved first. The Define
  Loading dialog says 'For a "One Load" analysis, only use the left side'. This rebuild now does
  the same: all six values in the dialog and file; the constant moment and shear are raised
  together from zero to their values and held (failing there gives "the constant loads alone
  can't be carried ... lower the constant loads"); then the load factor on all three increments
  is raised until failure, and the factor at the peak is reported. No increments = a one-load
  analysis. Sample beam: M/V 1.5 m (MM 1.5, MV 1) 182 kN; constant 100 kNm + shear increment
  341 kN; constant 100 kN shear + moment increment 324 kNm; moment increment only 360 kNm
  (moment-curvature peak 355); constant 400 kNm refused (fails at 90 % of it).
* Constant + increment loading (like the original's Define Loading dialog: constant N, M, V plus
  increments): the constant axial load and a constant moment are applied first and held, then the
  shear rises with the moment increment per kN of shear (file: BN, BM constant; MM/MV increment).
  The constant-moment start is read off the moment-curvature curve and balanced with the full
  section model at zero shear; only the strain-driven solver supports it. Not supported yet: a
  constant shear, an axial-load increment, and a constant moment opposite to the moment
  increment. Sample beam, largest shear at a fixed moment (increment = shear only): 0 kNm 305 kN,
  50 kNm 241, 100 kNm 348, 200 kNm 324, 300 kNm 126 (not yet compared with the original; the
  50 kNm value looks low against its neighbours).
* Cross-check of doubtful M-V points (unconfirmed, flagged or unclear, between 2 % and 97 % of
  the bending capacity): the point's moment is applied and held and only the shear is raised.
  Agreement within 5 % (both runs ending in real failure) confirms the point; if the proportional
  run had stopped without failure evidence and the fixed-moment run fails for real at a higher
  shear, the point moves to it; otherwise the point stays hollow (the paths disagree). What the
  comparison showed and what was changed in the solvers:
  - proportional runs at high moment stalled where the tension bars reach yield (load control
    can't climb a plateau). Now, when no higher load balances and tension-side bars have yielded,
    the run switches to curvature control and goes on until the shear has fallen 15 % from its
    peak or the section breaks (ratio 8 m: 40 kN at 321 kNm before, 44 kN at 355 kNm after);
  - a run that stalls at diagonal cracking falls back to the load-stepping solver (near
    M/V 0.18 m: 221 kN before, 351 kN after);
  - the two paths agree within 5 % at most points (e.g. -59.7 kNm: 204.5 / 205.0 kN; 261 kNm:
    236.0 / 238.2 kN) and still disagree near -113, -82, 273 and 324 kNm, shown hollow.
  Full diagram for the sample beam with the checks: about 33 s.
* Comparison with the original (M-V view, and the text report for the sample beam): the
  original's envelope is drawn over this program's, with a table of the shear at each of the
  original's moments and the two bending capacities. Built-in readings for the sample beam
  (taken off the original's M-V plot), or any "moment, shear" text file. Current result for the
  sample beam: -80 kNm 188 / 154.7 kN (-17.7 %), 0: 290 / 304.5 (+5.0 %), 68: 370 / 357.7
  (-3.3 %), 105: 390 / 395.8 (+1.5 %), 157: 345 / 365.5 (+5.9 %), 205: 320 / 332.9 (+4.0 %);
  bending capacity -117 / -128.1 kNm (+9.5 %), 338 / 354.8 kNm (+5.0 %); mean 6.5 %. The
  readings are off a plot (about +-5 kN, +-3 kNm), not the original's exported numbers.
* Load units: kN, kNm.
* M-V envelope smoothing review (2026-11-24). Kinks and dips in the M-V plot came from the
  diagram-building layer, not only from the solver: (1) every point had the same 8 % "inside"
  tolerance, so solver stops 3-8 % short stayed on the line as saw-teeth; (2) the re-meshing
  kept the largest *proven* run even when another mesh balanced a clearly higher load, so
  neighbouring ratios landed on different meshes; (3) cross-checks that reached further out were
  ignored unless they had failure proof and the proportional run had none; (4) the cracking and
  yield curves skipped runs without the event and joined the rest, so the yield line was drawn
  straight across the shear-dominated runs; (5) the radial ("original's run layout") M-V had no
  envelope test at all. Changes, all on the principle already used in the solver (no unloading
  in the material model, so any balanced state is a load the section carries):
  - `inside_flags(points, sure)`: tolerance 8 % for a confirmed point, 3 % for an unconfirmed
    one, measured against the chord between the neighbours (not r linear in angle, which
    overshoots across a gap left by dropped points and dropped confirmed points beside it);
  - `with_mesh_retry_sel(.., outermost = true)` for interaction points: the outermost run of all
    meshes is kept, confirmed if it or a proven run within 3 % has failure proof (single
    sectional analyses keep the old rule);
  - cross-checks: a point moves out to whichever other loading path reached > 3 % further, with
    or without failure proof (confirmed only with it);
  - the envelope is returned in direction order, so a moved point can't make the line double back;
  - event curves keep a NaN gap per run without the event; plots split lines at NaN;
  - the radial M-V view uses the same envelope test (grey x for early stops).
  Roughness (each envelope point against the line through its neighbours, scaled polar plot):
  sample beam 20 points per side, mean 1.91 % -> 1.30 %, kinks over 3 %: 10 -> 5, worst dip
  -5.4 % -> -3.3 %; T-beam (shots/cases/quick-tee.rsp) 16 per side, mean 1.80 % -> 1.09 %,
  kinks 7 -> 2, worst spike +7.2 % -> +2.1 %. Pictures: shots/mv-smooth-before.png,
  shots/mv-smooth-after.png (12 per side). Still open, in the solver: runs in the zone of
  largest shear stall just after diagonal cracking on every mesh (T-beam: M = -24 to +58 kNm stop
  at 232-264 kN while -55 kNm reaches 377), so the envelope there is a chord between computed
  points, not a computed value; and near the bending capacity the curvature-controlled runs stop
  on the yield plateau (sample beam ~327 kNm for V 65-140 kN, then 356 kNm at V 43).
* Strain-driven solver past diagonal cracking (2026-11-24). Why runs in the zone of largest
  shear stopped just after diagonal cracking (sample beam M/V 0.15-0.25 m at 241-320 kN on every
  mesh; prestressed T-beam M/V 0-0.4 m at 238-275 kN): a single layer's response is continuous
  (T-beam web layer: 1.86 MPa at cracking, 0.17 mm/m; dip to 1.56 MPa; stirrups reload it past
  3.8 MPa), but when several web layers reach that peak together each layer's "carry your share"
  equation has three answers (before the peak, in the dip, on the stirrup branch), and the
  "scaled" jump guesses (x1.3 ... x40 of the fast layers, crack openings, curvature jumps) mix
  branches between layers, so the whole-system corrector never converged. Changes (path.rs):
  - `branch_guess` / `branch_layers`: first rescue when a load step can't be balanced. Each
    layer's shear strain is marched from where it is to the nearest strain that carries its
    share of the new load (through the dip onto the stirrup branch when it must), its transverse
    strain solved with it; every layer is then on one consistent branch;
  - `correct_condensed`: Newton on the plane strain (ec, phi) only, every layer re-solved this
    way at each trial plane, then the full corrector from there (it accepts at once). Layers only
    interact through the plane strain, so this is far better behaved than the 2n+3 system. Also
    tries the smallest curvature jump (x1.3 ... x8) that balances: in the prestressed T-beam the
    top flange is in tension at its cracking stress and cracks while the web takes its shear;
  - shares on this path: relaxation halved when their change grows, raised again (x1.3, up to
    0.6) while it shrinks; with layers on a cracking knife-edge they can cycle with no exact
    fixed point, so the best of 40 passes is kept when its shares moved by under 5 % of the
    shear (the older solver accepted 4 blended passes unchecked); the state is balanced exactly
    for the shares it used, and the next step goes on converging them.
  The rescue is used only while the shear rises (not for moment-only increments) and before any
  longitudinal bar has yielded: tried everywhere, it also carried runs across the bars' yield
  plateau onto strain hardening (constant 100 kN shear, moment raised: 354 kNm against the
  original's 328, -0.2 -> +7.8 %); the plateau keeps its curvature-controlled continuation.
  SR_NO_BRANCH_GUESS=1 turns the rescue off (the previous solver). SR_CAND=1 traces it;
  SR_LAYER_I / SR_LAYER_EX pick the layer and strains for the SR_LAYER sweep.
  Single runs, one mesh (kN), before / after: sample beam M/V 0.15: 243 / 355 (original 352);
  0.18: 241 / 369; 0.2: 246 / 374 (now ends at 25 mm/m shear strain); 0.25: 320 / 373;
  0.3: 351 / 354; 0.6: 320 / 338; 0, 0.08, 1, 2, 4 m unchanged. T-beam (quick-tee.rsp) M/V
  0.1: 247 / 423; 0.2: 257 / 449; 0.4: 276 / 453; 1.0 m unchanged, 3.0 m +2 %.
  --check-mechanics: 20 of 20 runs within tolerance before and after, mean difference 2.45 ->
  2.43 % (constant 100 kNm, shear raised -8.8 -> -4.8 %; M/V 0.6 m +9.1 -> +11.5 %; N = -6000
  kN shear capacity -3.7 -> +4.3 %; the original's 7 sectional runs 4.6 -> 4.4 %). Full M-V
  diagrams: T-beam unconfirmed points 10 -> 2, early stops 5 -> 1, envelope kinks over 3 %
  2 -> 1; sample beam unconfirmed 9 -> 7, early stops 1 -> 0 (its kinks are now all in the
  yield-plateau shelf at 303-327 kNm). Still open: T-beam pure shear stops at 276 kN (M/V 0.1 m
  reaches 423) with the prestressed flange cracking and the shares cycling; near the bending
  capacity, runs stop on the bars' yield plateau (sample beam 303-327 kNm for V 113-161 kN,
  then 355 kNm at V 43), the largest step left in both diagrams.
* The yield-plateau "stops" near the bending capacity (2026-11-24). Sample beam M/V 4 m traced:
  past yield the run goes on under curvature control along the plateau (bar strain 2 -> 10
  mm/m, M about 326 kNm) and then every step fails. Cause: with the bars on the plateau the
  change of bar force between the two sections of the dual section is zero, so the shear flow
  moves up into the compression zone (sample beam: 3.6 and 1.75 MPa in the top two layers, none
  in the cracked web - the extra moment comes from the compression zone moving). The top layers
  lie above the stirrups (rho_z = 0): their shear stress peaks at 2-3.6 MPa under 1.5-1.9 mm/m
  compression and then softens to nothing, with no stirrup branch to reload on, so once a
  layer's share passes its peak no balanced state exists. That is a real shear-compression
  failure of the model, not a solver stop, and the corner it makes in the envelope (sample beam:
  about 327 kNm for V 104-134 kN, 355 kNm at V 43, where the top layers stay below their peak
  and the bars reach strain hardening) is the model's own; the original shows the same kind of
  step (M/V 4 m: 349 kNm at V 87; constant 100 kN shear: 328 kNm), at a somewhat larger shear.
  Change: when curvature control past yield finds no further state, `layer_at_capacity` checks
  whether a layer already carries all the shear it can (no larger stress at any larger shear
  strain up to 25 mm/m); if so the run ends confirmed, "Shear capacity of the concrete at z = ...
  reached (outside the stirrups) past the bars' yield.", which counts as failure proof. No
  capacity changes (--check-mechanics 2.43 %, 20/20; two runs differ by under 0.2 kN because a
  proven run skips the other meshes). Sample beam M-V: unconfirmed points 7 -> 3 (the shelf is
  now confirmed); M/V 0.3, 3 and 4 m end with this reason. Tried and dropped: accepting the
  8-tolerance band when the corrector runs out of iterations (it crawls there): runs went only a
  little further and others moved to other branches (2.43 -> 2.59 %). Still unexplained: M/V 0.6,
  2.5 and 6 m end on the plateau with no layer at capacity; M/V 1.5 m fails in the web at yield
  (V 188.6, then falling with the curvature) against the original's 203.1.
* Remaining solver stops and the post-yield shear-share calibration (2026-11-24).
  Stops traced (sample beam, one mesh): no layer near its capacity (17-30 % reserve), the
  corrector stuck on one layer's transverse balance (M/V 2.5 m layer 35 at 8.2 tolerances, M/V
  6 m top layer at 5.0, i.e. 0.03-0.04 MPa) in layers compressed to 15-40 MPa, where the
  transverse response is nearly flat (M/V 6 m: the top layer at 39.8 MPa, f'c 40).
  - `within`: on curvature control past the bars' yield only, a layer's transverse balance is
    held to 0.5 % of its principal compressive stress where that is looser than the fixed
    tolerance. M/V 2.5 m now ends confirmed (concrete shear capacity at z = 518 mm) and 6 m at
    25 mm/m shear strain, capacities unchanged (130.4 -> 130.5, 54.5). Used everywhere it moved
    other runs to other branches (--check-mechanics 2.43 -> 2.53 %); scoped, the suite is 2.50 %
    (20/20), the whole change being one run: constant 100 kNm, shear raised, the built-in sample
    beam, now ends confirmed at the concrete capacity at z = 322 mm, 364.1 kN, instead of at
    z = 352 mm, 369.5 kN (-4.7 -> -6.1 % against the original). SR_ABS_TOL=1 turns it off.
  - `polish_transverse`: when the corrector stalls or runs out of iterations, every layer's
    transverse strain is solved exactly (`layer_solve`) and the corrector carries on if that
    helps. Neutral on the suite (2.43 % alone); it doesn't free these runs (the exact root of the
    stuck layer is a different root: its share equation jumps to 184 tolerances), kept as a cheap
    safeguard. SR_NO_POLISH=1 turns it off.
  - Still stopping: M/V 0.6 m (the moment equation and many web layers at once at the onset of
    yield, web shear strain 4.8 mm/m, every layer 25-30 % below its capacity) and M/V 8 m (the
    share of the bottom cover layer below the bars, z = 8 mm, no stirrups, cracked at 11 mm/m).
  Calibration review - the cap on a layer's shear stress (the original: 8 x "an average one",
  constant 8.0 decompiled, divisor unidentified; here 8 x V over the gross concrete area,
  **estimated**). It is what binds in the post-yield corner (M/V 4 m: the top layer at 3.61 MPa
  = 8 V / A). SR_SHARE_CAP sets the factor (default 8, study only). --check-mechanics at 4 / 6 /
  8 / 12: 2.21 / 2.49 / 2.50 / 2.50 %; only M/V 4 m moves (+0.5 % at 4, -6.5 % at 6 and above).
  Not adopted: the factor is the original's own constant, and a factor of 4 on the gross area
  means averaging over twice the section, which no reading of the original allows (the likely
  divisors, web area or b d, give a higher cap, not lower); in a thin-web section it would also
  push web shear into the flanges, which the suite does not cover. A likelier source of the
  difference: the original's routine takes "the strain change times each node's current
  stiffness" - if that is the secant stiffness (the classic MCFT form), a yielded bar keeps
  passing shear flow on its plateau, where this rebuild's difference of two full solutions
  passes none. Secant stiffness for the bars alone was tried and changed nothing (both sections
  are still balanced with the concrete's tangent response, so no flow crosses the web); testing
  it properly means a secant dual section throughout, concrete included - a reformulation of
  `flow_shape`, left for a separate study with the original's shear-stress profiles past yield
  (Response-2000's "Calculated Shear stress distribution" plot) as the reference.
* Secant dual section (study, 2026-11-24; SR_FLOW_SECANT, off by default). `secant_changes`:
  the shear-flow shape from a unit curvature change (centroid strain change keeping N) times
  each node's secant stiffness - a concrete layer's uniaxial stress over strain (Ec at zero,
  small once cracked), a bar's stress over total strain (prestrain included) - integrated with
  `flow_from_changes` and capped as before (`shares_from_tau`). The reading of the original's
  "linear strain change times each node's current stiffness" as the secant stiffness, the
  classic MCFT form. Effect at the post-yield end of M/V 4 m: the shear runs down the stirruped
  web (0.60 MPa from z = 98 to 502 mm) instead of 3.6 MPa in the top layer above the stirrups;
  88.3 kN at 352 kNm against the original's 87.4 at 349.3 (default 81.8, -6.4 %).
  Modes, --check-mechanics (20/20 within tolerance in all three):
  - off (default): 2.50 %;
  - SR_FLOW_SECANT=1, everywhere: 2.75 % - better after yield (M/V 4 m -6.5 -> +1.1 %, constant
    100 kNm -6.1 -> +0.4 %) but worse before it (strands only M/V 1.5 m +0.2 -> -9.0 %, shear
    only -1.2 -> -2.5 %; secant and tangent differ for cracked concrete and strands); the
    T-beam's pure shear 276 -> 428 kN (the prestressed flange no longer stalls it);
  - SR_FLOW_SECANT=yield, once a reinforcing bar (not a strand) has yielded: 2.20 % - M/V 4 m
    +1.1 %, constant 100 kNm +0.2 %, N = -6000 kN +4.3 -> +2.2 %, M/V 0.6 m +11.5 -> +10.7 %,
    CSA-factored -2.0 -> -1.2 %, prestressed runs unchanged; worse: constant 100 kN shear,
    moment raised -0.3 -> +7.1 % (351 kNm: it now reaches strain hardening; the original's step
    lies between V 87, 349 kNm, and V 100, 328 kNm), shear only -1.2 -> -2.5 %, M/V 1.5 m -5.1 ->
    -6.0 %. Single runs (sample beam): M/V 0.3 m 354 -> 384 (original about 381), 2.5 m +2 %,
    4 m +8 %, 6 m +9 %, others within 1 %; M/V 8 m now ends past its peak (was a solver stop),
    0.45 m becomes one (370.4, same capacity). T-beam within 1 % except 3.0 m -3.6 %.
    Full M-V diagrams (20 / 16 points per side): sample beam mean deviation from the
    neighbours' line 1.51 -> 0.72 %, kinks over 3 % 6 -> 3, unconfirmed 3 -> 0, against the
    original's plot 6.3 -> 5.2 % (the post-yield shelf at 303-327 kNm is gone, the envelope
    rounds over to the bending capacity); T-beam 1.15 -> 0.98 %, kinks 2 -> 5 (largest -5.3 %),
    unconfirmed 3 -> 4. Pictures shots/mv-secant-yield-sample.png, mv-secant-yield-tee.png.
* Default changed (2026-11-24): the shear-flow shape now uses the secant dual section once a
  reinforcing bar has yielded (the "yield" mode above); before yield it is unchanged.
  --check-mechanics at the new default: 2.20 % (20/20), identical to the study run.
  SR_FLOW_SECANT=off restores the earlier behaviour (M/V 4 m: 81.8 kN, ending at the concrete
  shear capacity above the stirrups); SR_FLOW_SECANT=1 uses the secant form throughout.
  Known cost: constant 100 kN shear, moment raised, +7.1 % against the original (was -0.3 %).
* "Constant 100 kN shear, moment raised": the original's 328.1 kNm is not its capacity
  (checked 2026-11-24). Its own run (shots/orig-v100.png) ends at 38.3 rad/km with the
  moment-curvature curve flat at about 328 and still rising at the last point (the dashed end
  line is at the final state, which is also the maximum); the bottom fibre is at 20.2 mm/m, so
  the bars have just entered strain hardening (15 mm/m). Its M/V 4 m run (V 87) also ends while
  rising, at 53.7 rad/km and 349.3 kNm; only its moment-only run shows a real peak (356.4 kNm at
  about 50 rad/km, falling to 65). So 328.1 is the point where the original's run stopped, a
  lower bound, not a failure.
  Independent check (tools/chord_check.rs, a plain fibre model with the same material inputs plus
  the truss term V cot(theta) of web compression, N = 0; nothing from this program's solvers):
  V = 0: 358.6 kNm (352.8 without tension stiffening) at 60 rad/km, bars 27.6 mm/m - the
  original's 356.4 at 27.7 mm/m. V = 100 kN: 355.4 / 354.7 / 353.8 / 351.6 / 350.1 / 346.2 kNm
  for theta 70 / 65 / 60 / 50 / 45 / 35 deg, each needing 64-75 rad/km - nowhere near 328.
  This program (secant dual section after yield, the default): 352.0 kNm at V 100 (single mesh;
  351.4 with re-meshing), bars 28.3 mm/m and 473 MPa, top -4.5 mm/m, web strut about 62 deg,
  stirrups yielded over most of the web, ending at 25 mm/m shear strain; by hand at 62 deg the
  web carries about 82 kN (stirrups) + 15 kN (crack interlock) = 97 kN, so the web and the
  flexural peak coincide here. Conclusion: about 346-355 kNm is the credible capacity; the +7.1 %
  against the original in --check-mechanics is the original stopping early, and the earlier
  -0.3 % agreement came from this program stopping on the yield plateau too.
* The original's "constant 100 kN shear, moment raised" (328.1 kNm) is now marked as a lower
  bound (`compare::Case::lower_bound`, `Case::scored_pct`): only a result below it counts as a
  difference. --check-mechanics shows the signed difference (+7.10 %) and scores the shortfall
  (0.00 %; JSON "reference_kind": "lower_bound", "scored_difference_pct"); 20/20 within
  tolerance, mean difference 2.20 -> 1.84 %. SR_CASES marks it "(lower bound)"; the app's table of
  the original's runs (cases_view) says "(at least)" with a hover note and colours by the scored
  difference (that view has no menu entry at present). It is kept out of
  `sample_original_runs`. The original's M/V 4 m run (349.3 kNm, V 87.4) also ends while rising
  (shots/orig-mv4.png) but is left as a value: this program is above it (+1.1 %) either way.
* Early stops at the bars' yield in shear-dominated runs (2026-11-26). A 600 x 600 column (f'c 35,
  8 x 25M, 15M closed ties @ 250, N = 0) showed a jagged M-V top: with one mesh (40 layers), runs
  at neighbouring ratios ended either at 622-662 kN, "no balanced state", or at 694-713 kN, "peak
  passed under curvature control", depending on which side of 0.98 ey the bottom bars were when
  load control stalled (M/V 0.13 m: 648.9 kN at 1.958 mm/m; 0.125 m: 704.0 kN). Traced causes and
  changes (path.rs, `load_levels_cap`), each with an off switch:
  - curvature control was only tried with a tension bar past 0.98 ey. Now, in runs whose moment
    grows with the load (M/V over 50 mm), when following the drop gets nowhere it is tried as
    well (SR_NO_CURV_RETRY=1 turns it off);
  - curvature control stepped the curvature along the last direction of the path, which was
    sometimes already turning back (shear strain falling as the load rose), so it walked
    backwards; it now always increases the curvature in its own sense (SR_CURV_TANGENT_SIGN=1);
  - a curvature-controlled run whose shear fell steadily (3 states or more in a row, 0.5 % or more
    below its top) before the steps stopped converging ends as a passed peak, confirmed, instead
    of an unconfirmed "solver stop" whose re-meshing then took the largest of all meshes (often
    the coarse 30-layer one) (SR_NO_SOFT_PEAK=1);
  - with a tension bar past 0.9 ey and the web cracked, in runs whose moment grows with the load,
    a load step that balances at a higher shear while the curvature and the total web shear
    strain both grow by less than a quarter of the load's relative rise is refused (another root,
    not the next state on the path: M/V 0.219 m crept from 634 to 648 kN at a constant
    3.61 rad/km, then stopped; 0.2195 m reached 690.9). Refused steps go to the smaller-step and
    curvature-control fallbacks from the last good state (SR_NO_STAGNANT=1). Limited to near
    yield because just after diagonal cracking the web settles for a few steps in the same way,
    and that is the loading path (M/V 0.10 m fell to 469 kN with the test applied there).
  Column, one mesh, before -> after: M/V 0.10 m 713.0 -> 713.0, 0.11 661.8 -> 704.2, 0.12 655.7 ->
  703.4, 0.13 648.9 -> 698.5, 0.15 637.7 -> 690.8, 0.18 622.5 -> 685.8, 0.2 635.9 -> 688,
  0.219 647.6 -> 690.7. Full diagram (20 points a side): the plateau and dips at 120-230 kNm
  (649-662 kN between 687 and 670) are gone; the largest kink left is about 2 % (M -153 kNm 700 kN
  against -111 kNm 688 kN, from re-meshing). --check-mechanics 1.98 -> 1.95 %, 20/20 (strands only
  M/V 1.5 m -0.64 -> +0.07 %; constant 100 kN shear, moment raised +7.10 -> +6.71 %).
  Still open: pure shear and M/V 0.053 m end at web crushing that depends on the mesh (40 layers
  734 / 693 kN, 30-80 layers up to 776 / 741); M/V 0.24-0.29 m stop on curvature control past
  yield with no layer at capacity (654-670 kN, about 3 % low), left to re-meshing; the end of the
  diagram near the bending capacity was labelled "web crushing" though it is flexural (fixed in
  the next entry).
* Failure labels on M-V points (2026-11-26, path.rs end of `load_levels_cap`; `analysis::mechanism`).
  The column above had every point from the bending capacity up to the high-shear top labelled
  "Web crushing". Two causes: the crushing test took any layer with f2 >= 0.85 of its softened
  strength, which near the bending capacity is the flexural compression zone (principal
  compression at 0-1 deg, f2 = the axial stress, about f'c); and "web" meant one layer's change of
  shear strain exceeding the extreme fibre's change of strain over the last step, but a web
  layer cracked to ex = 30-36 mm/m takes a large shear strain under 0.02 MPa (M/V 11.9 m: 6.4
  mm/m against 0.04). Now the web governs when the shear does more work than the bending from
  the state just before the peak to the end of the run (sum of layer shear stress x change of
  shear strain x area, against M x change of curvature + N x change of centroid strain), and
  crushing needs an inclined strut (10-80 deg) at 0.85 of its strength. Capacities unchanged
  (--check-mechanics identical). Column: M/V 0-0.2 m and 0.4 m web crushing (struts at the limit),
  0.3 and 0.5 m web shear, 0.6 m and up (M 314 kNm and more) bars strain-hardening; sample beam:
  web shear / crushing for -69 to 213 kNm, bars yielded and then hardening beyond. SR_MECH=1
  prints the two works and the number of struts at their limit.
