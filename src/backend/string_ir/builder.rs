use crate::symbol::{Symbol, Interner};
use super::types::LlvmType;
use super::value::Value;

pub struct IrBuilder<'i> {
    pub output:     String,
    pub globals:    String,
    next_temp:       u32,
    next_block:      u32,
    next_global:     u32,
    interner:        &'i Interner,
    terminated:      bool,
}


impl<'i> IrBuilder<'i> {
    pub fn new(interner: &'i Interner) -> Self {
        IrBuilder {
            output:     String::new(),
            globals:    String::new(),
            next_temp:   0,
            next_block:  0,
            next_global: 0,
            interner,
            terminated: false,
        }
    }
    fn fresh_temp(&mut self) -> Value {
        let id = self.next_temp;
        self.next_temp += 1;
        Value::Temp(id)
    }
    pub fn fresh_block(&mut self, prefix: &str) -> String {
        let id = self.next_block;
        self.next_block += 1;
        format!("{}{}", prefix, id)
    }

    pub fn resolve(&self, sym: Symbol) -> &str {
        self.interner.resolve(sym)
    }

    pub fn emit_label(&mut self, label: &str) {

        self.output.push_str(&format!("{}:\n", label));
        self.terminated = false;
    }
    pub fn emit_function_begin(
        &mut self,
        name: Symbol,
        params: &[(Symbol, LlvmType)],
        ret_ty: &LlvmType,
    ) {
        let name_str = self.interner.resolve(name);
        let params_str: Vec<String> = params
            .iter()
            .map(|(sym, ty)| format!("{} %{}", ty.to_ir_str(),
                                     self.interner.resolve(*sym)))
            .collect();
        self.output.push_str(&format!("define {} @{}({}) {{\n", ret_ty.to_ir_str(), name_str, params_str.join(", ")));
    }
    pub fn emit_function_end(&mut self) {
        self.output.push_str("}\n\n");
    }
    pub fn emit_struct_type(&mut self, name: Symbol, fields: &[LlvmType]) {
        //let name_str = self.interner.resolve(name);
        let fields_str: Vec<String> = fields.iter().map(|t| t.to_ir_str()).collect();
        self.globals.push_str(&format!("%struct.{} = type {{ {} }}\n",
                                       name.0, fields_str.join(", "),
        ));
    }
    pub fn emit_enum_type(&mut self, name: Symbol, payload_slots: usize) {
        self.globals.push_str(&format!(
            "%enum.{} = type {{ i32, [{} x i64] }}\n", name.0, payload_slots,
        ));
    }
    pub fn emit_variant_type(&mut self, enum_name: Symbol, variant: Symbol, fields: &[LlvmType]) {
        let fs: Vec<String> = fields.iter().map(|t| t.to_ir_str()).collect();
        self.globals.push_str(&format!(
            "%variant.{}.{} = type {{ {} }}\n", enum_name.0, variant.0, fs.join(", "),
        ));
    }
    pub fn build_gep_enum_tag(&mut self, enum_name: Symbol, ptr: Value) -> Value {
        let r = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = getelementptr %enum.{}, ptr {}, i32 0, i32 0\n",
            r.to_ir_str(), enum_name.0, ptr.to_ir_str(),
        ));
        r
    }
    pub fn build_gep_enum_payload(&mut self, enum_name: Symbol, ptr: Value) -> Value {
        let r = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = getelementptr %enum.{}, ptr {}, i32 0, i32 1\n",
            r.to_ir_str(), enum_name.0, ptr.to_ir_str(),
        ));
        r
    }
    pub fn build_gep_variant_field(&mut self, enum_name: Symbol, variant: Symbol, payload_ptr: Value, idx: u32) -> Value {
        let r = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = getelementptr %variant.{}.{}, ptr {}, i32 0, i32 {}\n",
            r.to_ir_str(), enum_name.0, variant.0, payload_ptr.to_ir_str(), idx,
        ));
        r
    }
    pub fn build_alloca(&mut self, ty: &LlvmType, _name: &str) -> Value {
        let ptr = Value::Temp(self.next_temp);
        self.next_temp += 1;
        self.output.push_str(&format!(
            "  {} = alloca  {}\n",
            ptr.to_ir_str(), ty.to_ir_str(),
        ));
        ptr
    }

    pub fn build_store(&mut self, ty: &LlvmType, val: Value, ptr: Value) {
        self.output.push_str(&format!(
            "  store {} {}, ptr {}\n",
            ty.to_ir_str(), val.to_ir_str(), ptr.to_ir_str(),
        ));
    }

    pub fn build_load(&mut self, ty: &LlvmType, ptr: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = load {}, ptr {}\n",
            result.to_ir_str(), ty.to_ir_str(), ptr.to_ir_str(),
        ));
        result
    }
    pub fn build_add(&mut self, ty: &LlvmType, lhs: Value, rhs: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = add {} {}, {}\n",
            result.to_ir_str(), ty.to_ir_str(), lhs.to_ir_str(), rhs.to_ir_str(),
        ));
        result
    }
    pub fn build_sub(&mut self, ty: &LlvmType, lhs: Value, rhs: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = sub {} {}, {}\n",
            result.to_ir_str(), ty.to_ir_str(), lhs.to_ir_str(), rhs.to_ir_str(),
        ));
        result
    }

    pub fn build_mul(&mut self, ty: &LlvmType, lhs: Value, rhs: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = mul {} {}, {}\n",
            result.to_ir_str(), ty.to_ir_str(), lhs.to_ir_str(), rhs.to_ir_str(),
        ));
        result
    }
    pub fn build_sdiv(&mut self, ty: &LlvmType, lhs: Value, rhs: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = sdiv {} {}, {}\n",
            result.to_ir_str(), ty.to_ir_str(), lhs.to_ir_str(), rhs.to_ir_str(),
        ));
        result
    }
    pub fn build_checked_sdiv(&mut self, ty: &LlvmType, lhs: Value, rhs: Value) -> Value {
        let is_zero = self.build_icmp("eq", ty, rhs.clone(), Value::Const(0));
        let panic_label = self.fresh_block("div_panic");
        let ok_label = self.fresh_block("div_ok");
        self.build_cond_br(is_zero, &panic_label, &ok_label);
        self.emit_label(&panic_label);
        self.output.push_str("  call void @lexora_panic(ptr @.panicmsg_div)\n");
        self.build_unreachable();
        self.emit_label(&ok_label);
        self.build_sdiv(ty, lhs, rhs)
    }

    pub fn build_checked_arith(&mut self, intrinsic: &str, ty: &LlvmType, lhs: Value, rhs: Value) -> Value {
        let t = ty.to_ir_str();
        let pair = self.fresh_temp();
        self.output.push_str(&format!("  {} = call {{{}, i1}} @llvm.{}.with.overflow.{}({} {}, {} {})\n",
        pair.to_ir_str(),t , intrinsic, t, t, lhs.to_ir_str(),t, rhs.to_ir_str()));
        let res = self.fresh_temp();
        self.output.push_str(&format!("  {} = extractvalue {{{}, i1}} {}, 0\n", res.to_ir_str(), t, pair.to_ir_str()));
        let ovf = self.fresh_temp();
        self.output.push_str(&format!("  {} = extractvalue {{{}, i1}} {}, 1\n", ovf.to_ir_str(), t, pair.to_ir_str()));
        let panic_label = self.fresh_block("ovf_panic");
        let ok_label = self.fresh_block("ovf_ok");
        self.build_cond_br(ovf, &panic_label, &ok_label);
        self.emit_label(&panic_label);
        self.output.push_str("  call void @lexora_panic(ptr @.panicmsg_ovf)\n");
        self.build_unreachable();
        self.emit_label(&ok_label);
        res
    }

    pub fn build_icmp(&mut self, pred: &str, ty: &LlvmType, lhs: Value, rhs: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = icmp {} {} {}, {}\n",
            result.to_ir_str(), pred, ty.to_ir_str(), lhs.to_ir_str(), rhs.to_ir_str(),
        ));
        result
    }
    pub fn build_and(&mut self, lhs: Value, rhs: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = and i1 {}, {}\n",
            result.to_ir_str(), lhs.to_ir_str(), rhs.to_ir_str(),
        ));
        result
    }
    pub fn build_or(&mut self, lhs: Value, rhs: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = or i1 {}, {}\n",
            result.to_ir_str(), lhs.to_ir_str(), rhs.to_ir_str(),
        ));
        result
    }
    pub fn build_not(&mut self, val: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = xor i1 {}, 1\n",
            result.to_ir_str(), val.to_ir_str(),
        ));
        result
    }
    pub fn build_neg(&mut self, ty:&LlvmType, val: Value) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = sub {} 0, {}\n",
            result.to_ir_str(), ty.to_ir_str(), val.to_ir_str(),
        ));
        result
    }
    pub fn build_sext(&mut self, val: Value, from: &LlvmType, to: &LlvmType) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = sext {} {} to {}\n",
            result.to_ir_str(), from.to_ir_str(), val.to_ir_str(), to.to_ir_str(),
        ));
        result
    }

    pub fn build_trunc(&mut self, val: Value, from: &LlvmType, to: &LlvmType) -> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = trunc {} {} to {}\n",
            result.to_ir_str(), from.to_ir_str(), val.to_ir_str(), to.to_ir_str(),
        ));
        result
    }
    pub fn build_br(&mut self, label: &str) {
        if self.terminated {return;}
        self.output.push_str(&format!("  br label %{}\n", label));
        self.terminated = true;
    }
    pub fn build_cond_br(&mut self, cond: Value, then_label: &str, else_label: &str) {
        if self.terminated {return;}
        self.output.push_str(&format!("  br i1 {}, label %{}, label %{}\n", cond.to_ir_str(), then_label, else_label));
        self.terminated = true;
    }
    pub fn build_ret(&mut self, ty: &LlvmType, val: Value) {
        if self.terminated {return;}
        if *ty == LlvmType::Void {
            self.output.push_str("  ret void\n");
        } else {
            self.output.push_str(&format!("  ret {} {}\n", ty.to_ir_str(), val.to_ir_str()));
        }
        self.terminated = true;
    }
    pub fn is_terminated(&self) -> bool {
        self.terminated
    }
    pub fn build_unreachable(&mut self) {
        if self.terminated { return; }
        self.output.push_str("  unreachable\n");
        self.terminated = true;
    }
    pub fn build_call(
        &mut self,
        ret_ty: &LlvmType,
        name: Symbol,
        args: &[(LlvmType,Value)],
    ) -> Value {
        let args_str: Vec<String> = args.iter().map(|(ty, val)| format!("{} {}", ty.to_ir_str(), val.to_ir_str())).collect();
        let name_str = self.interner.resolve(name).to_string();
        if *ret_ty == LlvmType::Void {
            self.output.push_str(&format!("  call void @{}({})\n", name_str, args_str.join(", ")));
            Value::Void
        }else {
            let result = self.fresh_temp();
            self.output.push_str(&format!(
                "  {} = call {} @{}({})\n",
                result.to_ir_str(), ret_ty.to_ir_str(), name_str, args_str.join(", ")
            ));
            result
        }
    }
    pub fn build_gep_array(
        &mut self,
        elem_ty: &LlvmType,
        array_size: usize,
        ptr: Value,
        index: Value,
    )-> Value {
        let result = self.fresh_temp();
        self.output.push_str(&format!(
            "  {} = getelementptr [{} x {}], ptr {}, i32 0, i32 {}\n",
            result.to_ir_str(), array_size, elem_ty.to_ir_str(), ptr.to_ir_str(), index.to_ir_str(),
        ));
        result
    }
    pub fn build_checked_gep_array(
        &mut self,
        elem_ty: &LlvmType,
        array_size: usize,
        ptr: Value,
        index: Value,
    ) -> Value {
        let oob = self.build_icmp("uge", &LlvmType::I32, index.clone(),
        Value::Const(array_size as i64));
        let panic_label = self.fresh_block("idx_panic");
        let ok_label = self.fresh_block("idx_ok");
        self.build_cond_br(oob, &panic_label, &ok_label);
        self.emit_label(&panic_label);
        self.output.push_str("  call void @lexora_panic(ptr @.panicmsg_idx)\n");
        self.build_unreachable();
        self.emit_label(&ok_label);
        self.build_gep_array(elem_ty, array_size, ptr, index)
    }
    pub fn build_gep_struct(&mut self, struct_name: Symbol, ptr: Value, field_index: u32,) -> Value {
        let result = self.fresh_temp();
        //let name_str =  self.interner.resolve(struct_name).to_string();
        self.output.push_str(&format!(
            "  {} = getelementptr %struct.{}, ptr {}, i32 0, i32 {}\n",
            result.to_ir_str(), struct_name.0, ptr.to_ir_str(), field_index,
        ));
        result
    }
    pub fn build_box(&mut self, pointee: &LlvmType, val: Value) -> Value {
        let pt = pointee.to_ir_str();
        let size = self.fresh_temp();
        self.output.push_str(&format!("  {} = ptrtoint ptr getelementptr ({}, ptr null, i32 1) to i64\n", size.to_ir_str(), pt, ));
        let raw = self.fresh_temp();
        self.output.push_str(&format!("  {} = call ptr @lexora_alloc(i64 {})\n", raw.to_ir_str(), size.to_ir_str(), ));
        self.output.push_str(&format!("  store {} {}, ptr {}\n", pt, val.to_ir_str(), raw.to_ir_str(), ));
        raw
    }
    
    pub fn build_free(&mut self, ptr: Value) {
        self.output.push_str(&format!("  call void @lexora_free(ptr {})\n", ptr.to_ir_str(), ));
    }
    pub fn add_string_global(&mut self, s: &str) -> (Value, usize) {
        let id = self.next_global;
        self.next_global +=1;
        let len = s.len() + 1;
        self.globals.push_str(&format!("@str{} = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n", id, len, s));
        (Value::Named(format!("@str{}", id)), len)
    }
    pub fn finish(&self) -> String {
        format!("{}\n{}", self.globals, self.output)
    }
}