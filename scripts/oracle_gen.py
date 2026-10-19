import random
import struct
import sys

WORDS = ["a", "ab", "abc", "xyz", "", "q w", "uzun-metin", "z", "aa", "b"]
LIMIT = 1_000_000


FLOATS = [
    ("0.0", 0.0), ("1.5", 1.5), ("(0.0 - 2.75)", -2.75), ("1e10", 1e10),
    ("(0.0 - 1e10)", -1e10), ("3e9", 3e9), ("1e30", 1e30), ("2147483647.9", 2147483647.9),
    ("(0.0 - 0.5)", -0.5), ("f_nan()", float("nan")), ("f_inf()", float("inf")),
    ("(0.0 - f_inf())", float("-inf")),
]
FCMP = {
    "==": lambda a, b: a == b, "!=": lambda a, b: a != b, "<": lambda a, b: a < b,
    ">": lambda a, b: a > b, "<=": lambda a, b: a <= b, ">=": lambda a, b: a >= b,
}


def fsat(x, bits):
    lo, hi = -(2 ** (bits - 1)), 2 ** (bits - 1) - 1
    if x != x:
        return 0
    if x == float("inf"):
        return hi
    if x == float("-inf"):
        return lo
    return max(lo, min(hi, int(x)))


def f32(x):
    return struct.unpack("f", struct.pack("f", x))[0]


def tdiv(a, b):
    q = abs(a) // abs(b)
    return q if (a < 0) == (b < 0) else -q


class Gen:
    def __init__(self, seed):
        self.r = random.Random(seed)
        self.env = {}
        self.arrs = {}
        self.sts = {}
        self.svars = {}
        self.fns = {}
        self.lines = []
        self.expect = []
        self.n = 0

    def fresh(self, p):
        self.n += 1
        return f"{p}{self.n}"

    def num(self, d):
        r = self.r
        if d == 0 or r.random() < 0.35:
            pick = ["lit"]
            if self.env:
                pick.append("var")
            if self.arrs:
                pick.append("arr")
            if self.sts:
                pick.append("fld")
            if self.fns and d > 0:
                pick.append("call")
            k = r.choice(pick)
            if k == "var":
                nm = r.choice(list(self.env))
                return nm, self.env[nm]
            if k == "arr":
                nm = r.choice(list(self.arrs))
                i = r.randrange(len(self.arrs[nm]))
                return f"{nm}[{i}]", self.arrs[nm][i]
            if k == "fld":
                nm = r.choice(list(self.sts))
                f = r.choice(["x", "y"])
                return f"{nm}.{f}", self.sts[nm][f]
            if k == "call":
                fn = r.choice(list(self.fns))
                a, av = self.num(d - 1)
                b, bv = self.num(d - 1)
                v = self.fns[fn](av, bv)
                if abs(v) > LIMIT:
                    return "3", 3
                return f"{fn}({a}, {b})", v
            v = r.randint(-30, 30)
            return str(v), v
        op = r.choice(["+", "-", "*", "/"])
        a, av = self.num(d - 1)
        b, bv = self.num(d - 1)
        if op == "/":
            if bv == 0:
                b, bv = "7", 7
            v = tdiv(av, bv)
        elif op == "+":
            v = av + bv
        elif op == "-":
            v = av - bv
        else:
            if abs(av) > 1500 or abs(bv) > 1500:
                op, v = "+", av + bv
            else:
                v = av * bv
        if abs(v) > LIMIT:
            return "5", 5
        return f"(({a}) {op} ({b}))", v

    def say(self, src, val):
        self.lines.append(f"  print({src});")
        self.expect.append(str(val))

    def stmt(self):
        r = self.r
        k = r.randint(0, 20)
        if k == 0:
            nm = self.fresh("a")
            s, v = self.num(2)
            self.lines.append(f"  let {nm}: i32 = {s};")
            self.env[nm] = v
        elif k == 1 and self.env:
            nm = r.choice(list(self.env))
            s, v = self.num(2)
            self.lines.append(f"  {nm} = {s};")
            self.env[nm] = v
        elif k == 2:
            s, v = self.num(2)
            self.say(s, v)
        elif k == 3:
            nm = self.fresh("r")
            n = r.randint(1, 3)
            srcs, vals = [], []
            for _ in range(n):
                s, v = self.num(1)
                srcs.append(s)
                vals.append(v)
            self.lines.append(f"  let {nm}: [i32; {n}] = [{', '.join(srcs)}];")
            self.arrs[nm] = vals
        elif k == 4 and self.arrs:
            nm = r.choice(list(self.arrs))
            i = r.randrange(len(self.arrs[nm]))
            s, v = self.num(1)
            self.lines.append(f"  {nm}[{i}] = {s};")
            self.arrs[nm][i] = v
        elif k == 5:
            nm = self.fresh("p")
            s1, v1 = self.num(1)
            s2, v2 = self.num(1)
            pk = r.randint(0, 2)
            if pk == 0:
                self.lines.append(f"  let {nm}: P = P {{ x: {s1}, y: {s2} }};")
                self.sts[nm] = {"x": v1, "y": v2}
            elif pk == 1:
                self.lines.append(f"  let {nm}: P = P::yap({s1}, {s2});")
                self.sts[nm] = {"x": v1, "y": v2}
            else:
                self.lines.append(f"  let {nm}: P = P::kare({s1});")
                self.sts[nm] = {"x": v1, "y": v1}
        elif k == 6:
            acc = self.fresh("w")
            m = r.randint(0, 4)
            i = self.fresh("i")
            self.lines.append(f"  let {acc}: i32 = 0;")
            self.lines.append(f"  let {i}: i32 = 0;")
            self.lines.append(f"  while {i} < {m} {{ {acc} = {acc} + {i}; {i} = {i} + 1; }}")
            self.say(acc, sum(range(m)))
            self.env[acc] = sum(range(m))
        elif k == 7:
            acc = self.fresh("s")
            m = r.randint(0, 4)
            self.lines.append(f"  let {acc}: i32 = 0;")
            self.lines.append(f"  for q in 0..{m} {{ {acc} = {acc} + q; }}")
            self.say(acc, sum(range(m)))
            self.env[acc] = sum(range(m))
        elif k == 8:
            a = r.randint(-50, 50)
            nm = self.fresh("big")
            self.lines.append(f"  let {nm}: i64 = ({a} as i64) * (1000000000 as i64);")
            self.say(nm, a * 1000000000)
            self.say(f"({nm} / (1000000000 as i64)) as i32", a)
        elif k == 9:
            w = r.choice(WORDS)
            nm = self.fresh("t")
            self.lines.append(f'  let {nm}: String = string("{w}");')
            self.svars[nm] = w
            self.say(f"len({nm})", len(w))
            self.say(nm, w)
            other = r.choice(WORDS)
            self.say(f'{nm} == "{other}"', 1 if w == other else 0)
            self.say(f'{nm} < "{other}"', 1 if w < other else 0)
            if r.random() < 0.35:
                cn = self.fresh("tc")
                self.lines.append(f"  let {cn}: String = clone({nm});")
                self.say(cn, w)
                self.say(f"len({cn})", len(w))
                self.say(nm, w)
                yeni = r.choice(WORDS)
                self.lines.append(f'  {nm} = {nm} + "{yeni}";')
                self.svars[nm] = w + yeni
                self.say(nm, w + yeni)
                self.say(cn, w)
            elif r.random() < 0.5:
                self.say(f'{nm} + "{other}"', w + other)
                del self.svars[nm]
            elif r.random() < 0.6:
                yeni = r.choice(WORDS)
                self.lines.append(f'  {nm} = string("{yeni}");')
                self.svars[nm] = yeni
                self.say(nm, yeni)
                self.say(f"len({nm})", len(yeni))
                if r.random() < 0.5:
                    self.lines.append(f'  {nm} = {nm} + "{other}";')
                    self.svars[nm] = yeni + other
                    self.say(nm, yeni + other)
        elif k == 10:
            which = r.choice(["Daire", "Dik", "Nokta"])
            a, b = r.randint(-20, 20), r.randint(-20, 20)
            ctor = {"Daire": f"Sekil::Daire({a})", "Dik": f"Sekil::Dik({a}, {b})", "Nokta": "Sekil::Nokta"}[which]
            nm = self.fresh("k")
            if which == "Daire" and r.random() < 0.5:
                ctor = f"Sekil::kur({a})"
            self.lines.append(f"  let {nm}: Sekil = {ctor};")
            self.lines.append(
                f"  match {nm} {{ Sekil::Daire(v) => print(v), Sekil::Dik(x, y) => print(x + y), Sekil::Nokta => print(0) }}"
            )
            self.expect.append(str({"Daire": a, "Dik": a + b, "Nokta": 0}[which]))
        elif k == 19:
            def tval():
                c = r.randint(0, 3)
                if c == 0:
                    v = r.randint(-20, 20)
                    return f"TK {{ k: {v} }}", (lambda v: (2 * v, lambda j: v + j))(v)
                if c == 1:
                    w = r.choice(WORDS)
                    n = r.randint(-20, 20)
                    return f'TS {{ ad: string("{w}"), n: {n} }}', (lambda w, n: (len(w) + n, lambda j: n - j))(w, n)
                if c == 2:
                    return "TE::TX", (1, lambda j: 1 - j)
                v = r.randint(-20, 20)
                return f"TE::TY({v})", (lambda v: (v + 3, lambda j: v + 3 - j))(v)
            src, (ta, tb) = tval()
            j = r.randint(-20, 20)
            self.say(f"t_topla({src}, {j})", ta + tb(j))
            s1, (a1, _) = tval()
            s2, (a2, _) = tval()
            self.say(f"t_fark({s1}, {s2})", a1 - a2)
            w = r.choice(WORDS)
            n = r.randint(-20, 20)
            ts = self.fresh("ts")
            self.lines.append(f'  let {ts}: TS = TS {{ ad: string("{w}"), n: {n} }};')
            self.say(f"{ts}.ta()", len(w) + n)
            self.say(f"{ts}.tn()", w + "?")
            self.say(f"t_ad({ts})", w + "?x")
        elif k == 18:
            g = self.fresh("g")
            v = r.randint(-50, 50)
            self.lines.append(f"  let {g}: GK<i32> = GK {{ v: {v} }};")
            self.say(f"{g}.v", v)
            v2 = r.randint(-50, 50)
            self.lines.append(f"  {g}.ayarla({v2});")
            self.say(f"{g}.v", v2)
            self.say(f"{g}.sabit()", 5)
            self.say(f"{g}.iki_sabit()", 10)
            self.say(f"{g}.sar({r.randint(-9, 9)})", 1)
            if r.random() < 0.5:
                w = r.choice(WORDS)
                gs = self.fresh("gs")
                self.lines.append(f'  let {gs}: GK<String> = GK {{ v: string("{w}") }};')
                self.say(f"{gs}.v", w)
                self.say(f"len({gs}.v)", len(w))
                self.say(f"{gs}.iki_sabit()", 10)
                self.say(f'{gs}.sar(string("{r.choice(WORDS)}"))', 1)
            else:
                a = r.randint(-30, 30)
                gc = self.fresh("gc")
                self.lines.append(f"  let {gc}: GC<i32, bool> = GC {{ a: {a}, b: true }};")
                self.say(f"{gc}.a", a)
                a2 = r.randint(-30, 30)
                self.lines.append(f"  {gc}.ilk_yaz({a2});")
                self.say(f"{gc}.a", a2)
                self.say(f"{gc}.b", 1)
                self.say(f"{gc}.kod2()", 5)
        elif k == 12 and self.sts:
            nm = r.choice(list(self.sts))
            pick = r.randint(0, 2)
            if pick == 0:
                self.say(f"{nm}.topla()", self.sts[nm]["x"] + self.sts[nm]["y"])
            elif pick == 1:
                d, dv = self.num(1)
                self.lines.append(f"  {nm}.ekle({d});")
                self.sts[nm]["x"] = self.sts[nm]["x"] + dv
                self.say(f"{nm}.x", self.sts[nm]["x"])
            else:
                a = r.randint(-60, 60)
                b = r.randint(-60, 60)
                self.lines.append(f"  {nm}.yenile({a}, {b});")
                self.sts[nm]["x"] = a
                self.sts[nm]["y"] = b
                self.say(f"{nm}.topla()", a + b)
        elif k == 16:
            lim = r.randint(2, 6)
            cut = r.randint(0, lim + 1)
            acc = self.fresh("bt")
            iv = self.fresh("bi")
            self.lines.append(f"  let {acc}: i32 = 0;")
            self.lines.append(f"  for {iv} in 0..{lim} {{")
            dead = f" {acc} = {acc} + 1000;" if r.random() < 0.5 else ""
            self.lines.append(f"    if {iv} == {cut} {{ break;{dead} }}")
            self.lines.append(f"    {acc} = {acc} + {iv};")
            self.lines.append("  }")
            v = 0
            for i in range(lim):
                if i == cut:
                    break
                v += i
            self.say(acc, v)
            self.env[acc] = v
        elif k == 17:
            lim = r.randint(2, 6)
            cut = r.randint(0, lim + 1)
            acc = self.fresh("ct")
            cn = self.fresh("cw")
            self.lines.append(f"  let {acc}: i32 = 0;")
            self.lines.append(f"  let {cn}: i32 = 0;")
            self.lines.append(f"  while {cn} < {lim} {{")
            self.lines.append(f"    {cn} = {cn} + 1;")
            dead = f" {acc} = {acc} + 1000;" if r.random() < 0.5 else ""
            self.lines.append(f"    if {cn} == {cut} {{ continue;{dead} }}")
            self.lines.append(f"    {acc} = {acc} + {cn};")
            self.lines.append("  }")
            v = 0
            n = 0
            while n < lim:
                n += 1
                if n == cut:
                    continue
                v += n
            self.say(acc, v)
            self.say(cn, n)
            self.env[acc] = v
            self.env[cn] = n
        elif k == 14:
            x = r.randint(0, 6)
            n = r.randint(2, 5)
            nm = self.fresh("ec")
            rn = self.fresh("er")
            self.lines.append(f"  let {nm}: i32 = {x};")
            self.lines.append(f"  let {rn}: i32 = 0;")
            self.lines.append(f"  if {nm} == 0 {{ {rn} = 100; }}")
            for i in range(1, n):
                self.lines.append(f"  else if {nm} == {i} {{ {rn} = {100 + i}; }}")
            self.lines.append(f"  else {{ {rn} = -1; }}")
            v = 100 + x if x < n else -1
            self.say(rn, v)
            self.env[nm] = x
            self.env[rn] = v
        elif k == 15:
            w = r.choice(WORDS)
            a = r.randint(-40, 40)
            b = r.randint(-40, 40)
            nm = self.fresh("pk")
            tn = self.fresh("pt")
            dn = self.fresh("pd")
            self.lines.append(
                f'  let {nm}: PK = PK {{ ad: string("{w}"), n: {a}, m: {b} }};'
            )
            if r.random() < 0.5:
                self.lines.append(f"  let {tn}: String = clone({nm}.ad);")
                self.say(f"len({nm}.ad)", len(w))
                self.say(f"{nm}.ad", w)
            else:
                self.lines.append(f"  let {tn}: String = {nm}.ad;")
            self.say(tn, w)
            self.say(f"{nm}.n", a)
            self.say(f"{nm}.n + {nm}.m", a + b)
            self.lines.append(f"  let {dn}: i32 = {nm}.m;")
            self.say(dn, b)
            self.say(f"len({tn})", len(w))
            self.env[dn] = b
        elif k == 13:
            pick = r.randint(0, 2)
            if pick == 0:
                v = r.randint(-40, 40)
                self.say(f"(Sekil::Daire({v})).alan()", v * v)
            elif pick == 1:
                a = r.randint(-500, 500)
                b = r.randint(-500, 500)
                self.say(f"(Sekil::Dik({a}, {b})).alan()", a + b)
            else:
                self.say("(Sekil::Nokta).alan()", 0)
        elif k == 20:
            src, v = r.choice(FLOATS)
            nm = self.fresh("fl")
            self.lines.append(f"  let {nm}: f64 = {src};")
            self.say(f"{nm} as i32", fsat(v, 32))
            self.say(f"{nm} as i64", fsat(v, 64))
            bsrc, bv = r.choice(FLOATS)
            op = r.choice(list(FCMP))
            self.say(f"{nm} {op} {bsrc}", 1 if FCMP[op](v, bv) else 0)
            if r.random() < 0.4:
                g = self.fresh("fs")
                self.lines.append(f"  let {g}: f32 = {nm} as f32;")
                self.say(f"{g} as i32", fsat(f32(v), 32))
                self.say(f"{g} != {g}", 1 if f32(v) != f32(v) else 0)
        else:
            has = r.random() < 0.6
            nm = self.fresh("o")
            if r.random() < 0.5:
                v = r.randint(-9, 9)
                ctor = f"Opt::Var({v})" if has else "Opt::<i32>::Yok"
                self.lines.append(f"  let {nm}: Opt<i32> = {ctor};")
                self.lines.append(f"  match {nm} {{ Opt::Var(t) => print(t), Opt::Yok => print(0) }}")
                self.expect.append(str(v if has else 0))
            else:
                w = r.choice(WORDS)
                ctor = f'Opt::Var(string("{w}"))' if has else "Opt::<String>::Yok"
                self.lines.append(f"  let {nm}: Opt<String> = {ctor};")
                self.lines.append(f'  match {nm} {{ Opt::Var(t) => print(t), Opt::Yok => print(len("")) }}')
                self.expect.append(w if has else "0")

    def build(self):
        r = self.r
        head = [
            "struct P { x: i32, y: i32 }",
            "fn f_nan() -> f64 { 0.0 / 0.0 }",
            "fn f_inf() -> f64 { 1.0 / 0.0 }",
            "struct PK { ad: String, n: i32, m: i32 }",
            "struct GK<T> { v: T }",
            "struct GC<A, B> { a: A, b: B }",
            "enum GOpt<T> { Var(T), Yok }",
            "impl<T> GK<T> { fn ayarla(self, x: T) -> void { self.v = x; } fn sabit(self) -> i32 { return 5; } }",
            "impl<T> GK<T> {"
            " fn iki_sabit(self) -> i32 { return self.sabit() + self.sabit(); }"
            " fn sar(self, x: T) -> i32 {"
            " let o: GOpt<T> = GOpt::Var(x);"
            " match o { GOpt::Var(_) => 1, GOpt::Yok => 0 } } }",
            "impl<A, B> GC<A, B> { fn ilk_yaz(self, x: A) -> void { self.a = x; } fn kod(self) -> i32 { return 4; } }",
            "impl<A, B> GC<A, B> { fn kod2(self) -> i32 { return self.kod() + 1; } }",
            "enum Sekil { Daire(i32), Dik(i32, i32), Nokta }",
            "trait TA { fn ta(self) -> i32; fn tb(self, k: i32) -> i32; }",
            "trait TN { fn tn(self) -> String; }",
            "struct TK { k: i32 }",
            "struct TS { ad: String, n: i32 }",
            "enum TE { TX, TY(i32) }",
            "impl TA for TK { fn ta(self) -> i32 { self.k * 2 } fn tb(self, k: i32) -> i32 { self.k + k } }",
            "impl TA for TS { fn ta(self) -> i32 { len(self.ad) + self.n } fn tb(self, k: i32) -> i32 { self.n - k } }",
            "impl TA for TE { fn ta(self) -> i32 { match self { TE::TX => 1, TE::TY(v) => v + 3 } }"
            " fn tb(self, k: i32) -> i32 { self.ta() - k } }",
            'impl TN for TS { fn tn(self) -> String { clone(self.ad) + "?" } }',
            "fn t_topla<T: TA>(a: T, k: i32) -> i32 { a.ta() + a.tb(k) }",
            "fn t_fark<A: TA, B: TA>(a: A, b: B) -> i32 { a.ta() - b.ta() }",
            'fn t_ad<T: TA + TN>(x: T) -> String { x.tn() + "x" }',
            "enum Opt<T> { Var(T), Yok }",
            "impl P { fn topla(self) -> i32 { self.x + self.y }"
            " fn ekle(self, d: i32) -> void { self.x = self.x + d; }"
            " fn yap(a: i32, b: i32) -> P { return P { x: a, y: b }; }"
            " fn kare(a: i32) -> P { return P::yap(a, a); }"
            " fn yenile(self, a: i32, b: i32) -> void { self = P { x: a, y: b }; } }",
            "impl Sekil { fn alan(self) -> i32 { match self {"
            " Sekil::Daire(rr) => rr * rr, Sekil::Dik(aa, bb) => aa + bb, Sekil::Nokta => 0 } }"
            " fn kur(v: i32) -> Sekil { return Sekil::Daire(v); } }",
        ]
        for fi in range(r.randint(0, 3)):
            p1, p2 = f"p{fi}a", f"p{fi}b"
            k = r.randint(0, 2)
            if k == 0:
                op = r.choice(["+", "-", "*"])
                head.append(f"fn g{fi}({p1}: i32, {p2}: i32) -> i32 {{ return (({p1}) {op} ({p2})); }}")
                self.fns[f"g{fi}"] = (lambda o: (lambda a, b: a + b if o == "+" else (a - b if o == "-" else a * b)))(op)
            elif k == 1:
                head.append(f"fn g{fi}({p1}: i32, {p2}: i32) -> i32 {{ if ({p1}) > ({p2}) {{ return {p1}; }} return {p2}; }}")
                self.fns[f"g{fi}"] = lambda a, b: a if a > b else b
            else:
                head.append(
                    f"fn g{fi}({p1}: i32, {p2}: i32) -> i32 {{ let t: i32 = 0; let i: i32 = 0;"
                    f" while i < 3 {{ t = t + ({p1}); i = i + 1; }} return t + ({p2}); }}"
                )
                self.fns[f"g{fi}"] = lambda a, b: 3 * a + b
        for _ in range(r.randint(5, 12)):
            self.stmt()
        src = "\n".join(head) + "\nfn main() -> i32 {\n" + "\n".join(self.lines) + "\n  return 0;\n}\n"
        return src, self.expect


if __name__ == "__main__":
    g = Gen(int(sys.argv[1]))
    src, expect = g.build()
    open(sys.argv[2], "w").write(src)
    open(sys.argv[3], "w").write("\n".join(expect) + ("\n" if expect else ""))
