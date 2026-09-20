import random
import sys

I32, I64, BOOL, STR = "i32", "i64", "bool", "str"
SCALARS = [I32, I64, BOOL]
KEYWORDS = {
    "let", "fn", "return", "if", "else", "true", "false", "while", "for", "in",
    "and", "or", "not", "as", "import", "struct", "box", "enum", "match", "impl",
    "self", "i32", "i64", "f32", "f64", "bool", "void", "str", "String", "print",
    "string", "len",
}
OWN_PREAMBLE = """struct OwnBox { ad: String }

struct PartK { ad: String, n: i32, m: i32 }

struct GKutu<T> { v: T }

struct GCift<A, B> { a: A, b: B }

enum GOpt<T> { Var(T), Yok }

impl<T> GKutu<T> {
    fn ayarla(self, x: T) -> void { self.v = x; }
    fn sabit(self) -> i32 { return 7; }
}

impl<T> GKutu<T> {
    fn iki_sabit(self) -> i32 { return self.sabit() + self.sabit(); }
    fn sar(self, x: T) -> i32 {
        let o: GOpt<T> = GOpt::Var(x);
        match o {
            GOpt::Var(_) => 1,
            GOpt::Yok => 0,
        }
    }
}

impl<A, B> GCift<A, B> {
    fn ilk_yaz(self, x: A) -> void { self.a = x; }
    fn kod(self) -> i32 { return 5; }
}

impl<A, B> GCift<A, B> {
    fn kod2(self) -> i32 { return self.kod() + 1; }
}

fn iki_kat(v: i32) -> i32 { v / 2 }

impl OwnBox {
    fn yenile(self, s: str) -> void { self = OwnBox { ad: string(s) }; }
    fn koy(self, s: str) -> void { self.ad = string(s); }
    fn goster(self) -> void { print(self.ad); }
}
"""


class Gen:
    def __init__(self, rng):
        self.rng = rng
        self.structs = []
        self.enums = []
        self.fns = []
        self.methods = []
        self.setters = {}
        self.ctors = {}
        self.loopvars = set()
        self.n = 0

    def mk_struct(self, sname, fields, env, depth):
        init = ", ".join(f"{n}: {self.expr(t, env, depth)}" for n, t in fields)
        lit = f"{sname} {{ {init} }}"
        ctor = self.ctors.get(sname)
        if ctor is None or self.rng.random() < 0.5:
            return lit
        args = ", ".join(self.expr(t, env, depth) for _, t in fields)
        return f"{sname}::{ctor}({args})"

    def fresh(self, p="v"):
        self.n += 1
        name = f"{p}{self.n}"
        while name in KEYWORDS:
            self.n += 1
            name = f"{p}{self.n}"
        return name

    def lit(self, ty):
        r = self.rng
        if ty == I32:
            return str(r.randint(-100, 100))
        if ty == I64:
            m = r.randint(2**31, 10**10)
            return str(m if r.random() < 0.5 else -m)
        if ty == BOOL:
            return r.choice(["true", "false"])
        if ty == STR:
            return '"' + r.choice(["a", "ab", "abc", "xyz", "", "q w", "uzun-metin"]) + '"'
        raise AssertionError(ty)

    def expr(self, ty, env, depth):
        r = self.rng
        cands = [lambda: self.lit(ty)]
        same = [n for n, t in env.items() if t == ty]
        if same:
            cands.append(lambda: r.choice(same))
        if depth > 0:
            if ty in (I32, I64):

                def arith():
                    ops = ["+", "-"] if ty == I64 else ["+", "-", "*"]
                    a = self.expr(ty, env, depth - 1)
                    b = self.expr(ty, env, depth - 1)
                    return f"(({a}) {r.choice(ops)} ({b}))"

                def mul64():
                    a = self.expr(I32, env, depth - 1)
                    b = self.expr(I32, env, depth - 1)
                    return f"((({a}) as i64) * (({b}) as i64))"

                def div():
                    a = self.expr(ty, env, depth - 1)
                    d = r.randint(1, 50)
                    rhs = f"({d} as i64)" if ty == I64 else str(d)
                    return f"(({a}) / {rhs})"

                def neg():
                    return f"(-({self.expr(ty, env, depth - 1)}))"

                def ifexp():
                    c = self.expr(BOOL, env, depth - 1)
                    a = self.expr(ty, env, depth - 1)
                    b = self.expr(ty, env, depth - 1)
                    return f"(if {c} {{ {a} }} else {{ {b} }})"

                def matchexp():
                    ename, variants = r.choice(self.enums)
                    vn, vtys = r.choice(variants)
                    args = ", ".join(self.expr(t, env, depth - 1) for t in vtys)
                    ctor = f"{ename}::{vn}" + (f"({args})" if vtys else "")
                    arms = []
                    for wn, wtys in variants:
                        binds = [self.fresh("m") for _ in wtys]
                        pat = f"{ename}::{wn}" + (f"({', '.join(binds)})" if binds else "")
                        e2 = dict(env)
                        for b, t in zip(binds, wtys):
                            e2[b] = t
                        arms.append(f"{pat} => {self.expr(ty, e2, 0)}")
                    return "(match " + ctor + " { " + ", ".join(arms) + " })"

                cands += [arith, div, neg, ifexp]
                if ty == I64:
                    cands += [mul64, lambda: f"(({self.expr(I32, env, depth-1)}) as i64)"]
                else:
                    cands.append(lambda: f"(({self.expr(I64, env, depth-1)}) as i32)")
                if self.enums:
                    cands.append(matchexp)
            if ty == BOOL:

                def cmp():
                    t = r.choice([I32, I64])
                    op = r.choice(["==", "!=", "<", ">", "<=", ">="])
                    a = self.expr(t, env, depth - 1)
                    b = self.expr(t, env, depth - 1)
                    return f"(({a}) {op} ({b}))"

                def logic():
                    op = r.choice(["and", "or"])
                    a = self.expr(BOOL, env, depth - 1)
                    b = self.expr(BOOL, env, depth - 1)
                    return f"(({a}) {op} ({b}))"

                cands += [cmp, logic, lambda: f"(not ({self.expr(BOOL, env, depth-1)}))"]
            for fname, (params, ret) in self.fns:
                if ret == ty:

                    def call(fname=fname, params=params):
                        args = ", ".join(self.expr(p, env, depth - 1) for p in params)
                        return f"{fname}({args})"

                    cands.append(call)
            for sname, fields in self.structs:
                for fn_, fty in fields:
                    if fty == ty:

                        def fld(sname=sname, fields=fields, fn_=fn_):
                            base = self.mk_struct(sname, fields, env, depth - 1)
                            return f"({base}).{fn_}"

                        cands.append(fld)
            for sname, mname, ptys, mret in self.methods:
                if mret == ty:

                    def meth(sname=sname, mname=mname, ptys=ptys):
                        fields = dict(self.structs)[sname]
                        base = self.mk_struct(sname, fields, env, depth - 1)
                        args = ", ".join(self.expr(t, env, depth - 1) for t in ptys)
                        return f"({base}).{mname}({args})"

                    cands.append(meth)
            if ty == I32:
                for ename, variants in self.enums:

                    def ecall(ename=ename, variants=variants):
                        vn, vtys = r.choice(variants)
                        args = ", ".join(self.expr(t, env, depth - 1) for t in vtys)
                        ctor = f"{ename}::{vn}" + (f"({args})" if vtys else "")
                        return f"({ctor}).kod()"

                    cands.append(ecall)
        return r.choice(cands)()

    def block(self, env, depth, budget, indent):
        r = self.rng
        pad = "    " * indent
        out = []
        env = dict(env)
        for _ in range(budget):
            k = r.randint(0, 16)
            if k <= 2:
                ty = r.choice(SCALARS)
                name = self.fresh()
                out.append(f"{pad}let {name}: {ty} = {self.expr(ty, env, depth)};")
                env[name] = ty
            elif k == 3 and [n for n in env if n not in self.loopvars]:
                name = r.choice([n for n in env if n not in self.loopvars])
                out.append(f"{pad}{name} = {self.expr(env[name], env, depth)};")
            elif k == 4:
                ty = r.choice([I32, I64, BOOL, STR])
                val = self.lit(STR) if ty == STR else self.expr(ty, env, depth)
                out.append(f"{pad}print({val});")
            elif k == 5:
                out.append(f"{pad}if {self.expr(BOOL, env, depth)} {{")
                out += self.block(env, depth - 1, max(1, budget // 2), indent + 1)
                out.append(f"{pad}}} else {{")
                out += self.block(env, depth - 1, max(1, budget // 2), indent + 1)
                out.append(f"{pad}}}")
            elif k == 6:
                v = self.fresh("i")
                out.append(f"{pad}for {v} in 0..{r.randint(0, 4)} {{")
                e2 = dict(env)
                e2[v] = I32
                self.loopvars.add(v)
                out += self.block(e2, depth - 1, max(1, budget // 2), indent + 1)
                out.append(f"{pad}}}")
            elif k == 7 and self.enums:
                ename, variants = r.choice(self.enums)
                name = self.fresh("e")
                vn, vtys = r.choice(variants)
                args = ", ".join(self.expr(t, env, depth) for t in vtys)
                ctor = f"{ename}::{vn}" + (f"({args})" if vtys else "")
                out.append(f"{pad}let {name}: {ename} = {ctor};")
                out.append(f"{pad}match {name} {{")
                for wn, wtys in variants:
                    binds = [self.fresh("b") for _ in wtys]
                    pat = f"{ename}::{wn}" + (f"({', '.join(binds)})" if binds else "")
                    e2 = dict(env)
                    for b, t in zip(binds, wtys):
                        e2[b] = t
                    out.append(f"{pad}    {pat} => print({self.expr(I32, e2, 1)}),")
                out.append(f"{pad}}}")
                out.append(f"{pad}print({name}.kod());")
            elif k == 8 and r.random() < 0.5:
                cnt = self.fresh("w")
                out.append(f"{pad}let {cnt}: i32 = 0;")
                out.append(f"{pad}while {cnt} < {r.randint(0, 3)} {{")
                e2 = dict(env)
                e2[cnt] = I32
                out += self.block(e2, depth - 1, 1, indent + 1)
                out.append(f"{pad}    {cnt} = {cnt} + 1;")
                out.append(f"{pad}}}")
                out.append(f"{pad}print({cnt});")
                env[cnt] = I32
            elif k == 8:
                sv = self.fresh("s")
                out.append(f"{pad}let {sv}: String = string({self.lit(STR)});")
                choice = r.randint(0, 5)
                if choice == 4:
                    out.append(f"{pad}{sv} = string({self.lit(STR)});")
                    out.append(f"{pad}print({sv});")
                    out.append(f"{pad}{sv} = {sv} + {self.lit(STR)};")
                    out.append(f"{pad}print(len({sv}));")
                elif choice == 0:
                    out.append(f"{pad}print(len({sv}));")
                    out.append(f"{pad}print({sv});")
                elif choice == 1:
                    out.append(f"{pad}print({sv} == {self.lit(STR)});")
                elif choice == 2:
                    bv = self.fresh("s")
                    out.append(f"{pad}let {bv}: String = {sv} + {self.lit(STR)};")
                    out.append(f"{pad}print({bv});")
                elif choice == 5:
                    cv = self.fresh("cs")
                    out.append(f"{pad}let {cv}: String = clone({sv});")
                    out.append(f"{pad}print({sv});")
                    out.append(f"{pad}print({cv});")
                    out.append(f"{pad}{sv} = {sv} + {self.lit(STR)};")
                    out.append(f"{pad}print({sv});")
                    out.append(f"{pad}print(len({cv}));")
                else:
                    bx = self.fresh("bx")
                    out.append(f"{pad}let {bx}: Box<i32> = box {self.expr(I32, env, 1)};")
                    out.append(f"{pad}print(*{bx});")
            elif k == 16:
                gv = self.fresh("gk")
                ty = r.choice([I32, I64, BOOL, STR])
                out.append(
                    f"{pad}let {gv}: GKutu<{ty}> = GKutu {{ v: {self.expr(ty, env, 1)} }};"
                )
                out.append(f"{pad}print({gv}.v);")
                if r.random() < 0.6:
                    out.append(f"{pad}{gv}.ayarla({self.expr(ty, env, 1)});")
                    out.append(f"{pad}print({gv}.v);")
                    out.append(f"{pad}print({gv}.sabit());")
                out.append(f"{pad}print({gv}.iki_sabit());")
                out.append(f"{pad}print({gv}.sar({self.expr(ty, env, 1)}));")
                if r.random() < 0.5:
                    gc = self.fresh("gc")
                    t2 = r.choice([I32, BOOL])
                    out.append(
                        f"{pad}let {gc}: GCift<{ty}, {t2}> = GCift {{"
                        f" a: {self.expr(ty, env, 1)}, b: {self.expr(t2, env, 1)} }};"
                    )
                    out.append(f"{pad}{gc}.ilk_yaz({self.expr(ty, env, 1)});")
                    out.append(f"{pad}print({gc}.a);")
                    out.append(f"{pad}print({gc}.b);")
                    out.append(f"{pad}print({gc}.kod2());")
                else:
                    gs = self.fresh("gs")
                    out.append(
                        f"{pad}let {gs}: GKutu<String> = GKutu {{ v: string({self.lit(STR)}) }};"
                    )
                    out.append(f"{pad}print({gs}.v);")
                    out.append(f"{pad}print(len({gs}.v));")
                    out.append(f"{pad}print({gs}.iki_sabit());")
                    out.append(f"{pad}print({gs}.sar(string({self.lit(STR)})));")
            elif k == 9 and self.structs:
                sname, fields = r.choice(self.structs)
                sv = self.fresh("sv")
                base = self.mk_struct(sname, fields, env, depth - 1)
                out.append(f"{pad}let {sv}: {sname} = {base};")
                setter, sfield, sty = self.setters[sname]
                out.append(f"{pad}{sv}.{setter}({self.expr(sty, env, depth-1)});")
                out.append(f"{pad}print({sv}.{sfield});")
                own = [m for m in self.methods if m[0] == sname]
                if own:
                    _, mn, ptys, _ = r.choice(own)
                    args = ", ".join(self.expr(t, env, depth - 1) for t in ptys)
                    out.append(f"{pad}print({sv}.{mn}({args}));")
            elif k == 11:
                ob = self.fresh("ob")
                out.append(f"{pad}let {ob}: OwnBox = OwnBox {{ ad: string({self.lit(STR)}) }};")
                out.append(f"{pad}{ob}.goster();")
                for _ in range(r.randint(1, 3)):
                    if r.random() < 0.5:
                        out.append(f"{pad}{ob}.yenile({self.lit(STR)});")
                    else:
                        out.append(f"{pad}{ob}.koy({self.lit(STR)});")
                    out.append(f"{pad}{ob}.goster();")
            elif k == 12:
                rs = self.fresh("rs")
                rn = self.fresh("rn")
                out.append(f"{pad}let {rs}: String = string({self.lit(STR)});")
                out.append(f"{pad}let {rn}: i32 = 0;")
                out.append(f"{pad}while {rn} < {r.randint(1, 3)} {{")
                out.append(f"{pad}    print({rs});")
                out.append(f"{pad}    {rs} = {rs} + {self.lit(STR)};")
                out.append(f"{pad}    {rn} = {rn} + 1;")
                out.append(f"{pad}}}")
                out.append(f"{pad}print({rs});")
            elif k == 13:
                ec = self.fresh("ec")
                out.append(f"{pad}let {ec}: i32 = {self.expr(I32, env, depth)};")
                e2 = dict(env)
                e2[ec] = I32
                levels = r.randint(2, 5)
                if r.random() < 0.5:
                    er = self.fresh("er")
                    out.append(f"{pad}let {er}: i32 = 0;")
                    out.append(f"{pad}if {ec} == 0 {{ {er} = 100; }}")
                    for i in range(1, levels):
                        out.append(f"{pad}else if {ec} == {i} {{ {er} = {100 + i}; }}")
                    out.append(f"{pad}else {{ {er} = -1; }}")
                    out.append(f"{pad}print({er});")
                    env[ec] = I32
                    env[er] = I32
                else:
                    ev = self.fresh("ev")
                    head = f"{pad}let {ev}: i32 = if {ec} == 0 {{ 100 }}"
                    for i in range(1, levels):
                        head += f" else if {ec} == {i} {{ {100 + i} }}"
                    head += " else { -1 };"
                    out.append(head)
                    out.append(f"{pad}print({ev});")
                    env[ec] = I32
                    env[ev] = I32
            elif k == 15:
                bt = self.fresh("bt")
                bi = self.fresh("bi")
                lim = r.randint(2, 5)
                cut = r.randint(0, lim)
                out.append(f"{pad}let {bt}: i32 = 0;")
                if r.random() < 0.5:
                    out.append(f"{pad}for {bi} in 0..{lim} {{")
                    if r.random() < 0.5:
                        out.append(f"{pad}    if {bi} == {cut} {{ break; }}")
                    else:
                        out.append(f"{pad}    if {bi} == {cut} {{ continue; }}")
                    out.append(f"{pad}    {bt} = {bt} + 1;")
                    out.append(f"{pad}}}")
                else:
                    bw = self.fresh("bw")
                    out.append(f"{pad}let {bw}: i32 = 0;")
                    out.append(f"{pad}while {bw} < {lim} {{")
                    out.append(f"{pad}    {bw} = {bw} + 1;")
                    out.append(f"{pad}    if {bw} == {cut} {{ continue; }}")
                    out.append(f"{pad}    if {bw} > {lim} {{ break; }}")
                    out.append(f"{pad}    {bt} = {bt} + 1;")
                    out.append(f"{pad}}}")
                    env[bw] = I32
                out.append(f"{pad}print({bt});")
                env[bt] = I32
            elif k == 14:
                pk = self.fresh("pk")
                tn = self.fresh("pt")
                dn = self.fresh("pd")
                out.append(
                    f"{pad}let {pk}: PartK = PartK {{ ad: string({self.lit(STR)}),"
                    f" n: {self.expr(I32, env, 1)}, m: {self.expr(I32, env, 1)} }};"
                )
                if r.random() < 0.5:
                    out.append(f"{pad}let {tn}: String = clone({pk}.ad);")
                    out.append(f"{pad}print(len({pk}.ad));")
                else:
                    out.append(f"{pad}let {tn}: String = {pk}.ad;")
                out.append(f"{pad}print({tn});")
                out.append(f"{pad}print({pk}.n);")
                out.append(f"{pad}let {dn}: i32 = {pk}.m;")
                out.append(f"{pad}print({dn});")
                if r.random() < 0.5:
                    out.append(f"{pad}print(iki_kat({pk}.n));")
                else:
                    out.append(f"{pad}let {self.fresh('pa')}: [i32; 2] = [{pk}.n, {pk}.m];")
                env[dn] = I32
            else:
                name = self.fresh("a")
                n = r.randint(1, 3)
                ty = r.choice([I32, I64])
                elems = ", ".join(self.expr(ty, env, depth) for _ in range(n))
                out.append(f"{pad}let {name}: [{ty}; {n}] = [{elems}];")
                idx = r.randint(0, n - 1)
                out.append(f"{pad}{name}[{idx}] = {self.expr(ty, env, depth)};")
                out.append(f"{pad}print({name}[{idx}]);")
        return out

    def program(self):
        r = self.rng
        out = []
        out.append(OWN_PREAMBLE)
        for si in range(r.randint(0, 2)):
            fields = [(f"f{j}", r.choice(SCALARS)) for j in range(r.randint(1, 3))]
            name = f"S{si}"
            self.structs.append((name, fields))
            out.append(f"struct {name} {{ " + ", ".join(f"{n}: {t}" for n, t in fields) + " }")
            out.append("")
        for ei in range(r.randint(0, 2)):
            name = f"E{ei}"
            variants = []
            for vj in range(r.randint(1, 3)):
                tys = [r.choice(SCALARS) for _ in range(r.randint(0, 2))]
                variants.append((f"V{vj}", tys))
            self.enums.append((name, variants))
            vs = ", ".join(v + (f"({', '.join(t)})" if t else "") for v, t in variants)
            out.append(f"enum {name} {{ {vs} }}")
            out.append("")
        for sname, fields in self.structs:
            body = []
            for mj in range(r.randint(1, 2)):
                fn_, fty = r.choice(fields)
                mname = f"{sname.lower()}m{mj}"
                op = "and" if fty == BOOL else "+"
                body.append(
                    f"    fn {mname}(self, q: {fty}) -> {fty} {{ return self.{fn_} {op} q; }}"
                )
                self.methods.append((sname, mname, [fty], fty))
            ctor = f"{sname.lower()}yap"
            cparams = ", ".join(f"p{i}: {t}" for i, (_, t) in enumerate(fields))
            cinit = ", ".join(f"{n}: p{i}" for i, (n, _) in enumerate(fields))
            body.append(
                f"    fn {ctor}({cparams}) -> {sname} {{ return {sname} {{ {cinit} }}; }}"
            )
            self.ctors[sname] = ctor
            sfield, sty = fields[0]
            setter = f"{sname.lower()}set"
            body.append(f"    fn {setter}(self, v: {sty}) -> void {{ self.{sfield} = v; }}")
            self.setters[sname] = (setter, sfield, sty)
            out.append(f"impl {sname} {{")
            out += body
            out.append("}")
            out.append("")
        for ename, variants in self.enums:
            arms = []
            for vi, (vn, vtys) in enumerate(variants):
                binds = [f"z{vi}_{j}" for j in range(len(vtys))]
                pat = ename + "::" + vn + (("(" + ", ".join(binds) + ")") if binds else "")
                i32b = [b for b, t in zip(binds, vtys) if t == I32]
                arms.append(pat + " => " + (" + ".join(i32b) if i32b else str(vi)))
            out.append(
                "impl " + ename + " { fn kod(self) -> i32 { match self { "
                + ", ".join(arms)
                + " } } }"
            )
            out.append("")
        for fi in range(r.randint(0, 3)):
            fname = f"fn{fi}"
            params = [r.choice(SCALARS) for _ in range(r.randint(0, 3))]
            ret = r.choice(SCALARS)
            env = {}
            plist = []
            for pi, pt in enumerate(params):
                pn = f"p{fi}_{pi}"
                env[pn] = pt
                plist.append(f"{pn}: {pt}")
            body = self.block(env, 2, r.randint(0, 2), 1)
            body.append(f"    return {self.expr(ret, env, 2)};")
            out.append(f"fn {fname}({', '.join(plist)}) -> {ret} {{")
            out += body
            out.append("}")
            out.append("")
            self.fns.append((fname, (params, ret)))
        out.append("fn main() -> i32 {")
        out += self.block({}, 2, r.randint(2, 5), 1)
        out.append("    return 0;")
        out.append("}")
        return "\n".join(out) + "\n"


if __name__ == "__main__":
    print(Gen(random.Random(int(sys.argv[1]))).program(), end="")
