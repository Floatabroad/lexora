import random
import sys

WORDS = ["a", "ab", "abc", "xyz", "", "q w", "uzun-metin", "z", "aa", "b"]
LIMIT = 1_000_000


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
        k = r.randint(0, 13)
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
            self.lines.append(f"  let {nm}: P = P {{ x: {s1}, y: {s2} }};")
            self.sts[nm] = {"x": v1, "y": v2}
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
            if r.random() < 0.5:
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
            self.lines.append(f"  let {nm}: Sekil = {ctor};")
            self.lines.append(
                f"  match {nm} {{ Sekil::Daire(v) => print(v), Sekil::Dik(x, y) => print(x + y), Sekil::Nokta => print(0) }}"
            )
            self.expect.append(str({"Daire": a, "Dik": a + b, "Nokta": 0}[which]))
        elif k == 12 and self.sts:
            nm = r.choice(list(self.sts))
            if r.random() < 0.5:
                self.say(f"{nm}.topla()", self.sts[nm]["x"] + self.sts[nm]["y"])
            else:
                d, dv = self.num(1)
                self.lines.append(f"  {nm}.ekle({d});")
                self.sts[nm]["x"] = self.sts[nm]["x"] + dv
                self.say(f"{nm}.x", self.sts[nm]["x"])
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
            "enum Sekil { Daire(i32), Dik(i32, i32), Nokta }",
            "enum Opt<T> { Var(T), Yok }",
            "impl P { fn topla(self) -> i32 { self.x + self.y }"
            " fn ekle(self, d: i32) -> void { self.x = self.x + d; } }",
            "impl Sekil { fn alan(self) -> i32 { match self {"
            " Sekil::Daire(rr) => rr * rr, Sekil::Dik(aa, bb) => aa + bb, Sekil::Nokta => 0 } } }",
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
