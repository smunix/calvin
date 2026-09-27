use crate::lang::types::{MonoType, Prim};

/// Pretty-prints a runtime value returned by JIT compilation given its statically inferred `MonoType`.
///
/// # Safety
/// For heap-allocated types (Records, Tuples, Variants, Arrays), `raw` must be a valid pointer
/// allocated by `calvin_alloc`.
pub unsafe fn format_runtime_value<'a>(raw: u64, ty: &'a MonoType<'a>) -> String {
    match ty.chase() {
        MonoType::Prim(p) => match p {
            Prim::Unit => "()".to_string(),
            Prim::Bool => {
                if (raw as u8) != 0 {
                    "true".to_string()
                } else {
                    "false".to_string()
                }
            }
            Prim::Char => format!("'{}'", std::char::from_u32(raw as u32).unwrap_or('?')),
            Prim::Byte => (raw as u8).to_string(),
            Prim::Short => (raw as i16).to_string(),
            Prim::Int => (raw as i64).to_string(),
            Prim::Long => (raw as i64).to_string(),
            Prim::Int128 => (raw as i64).to_string(),
            Prim::Float | Prim::Double => {
                let f = f64::from_bits(raw);
                if f.fract() == 0.0 && f.abs() < 1e16 {
                    format!("{:.0}", f)
                } else {
                    f.to_string()
                }
            }
            Prim::Time | Prim::TimeSpan | Prim::DateTime => (raw as i64).to_string(),
        },
        MonoType::Record(fields, _) => {
            let ptr = raw as *const u8;
            if ptr.is_null() {
                return "{}".to_string();
            }
            let mut parts = Vec::new();
            for (i, (name, field_ty)) in fields.iter().enumerate() {
                let slot = ptr.add(i * 8);
                let val_raw = *(slot as *const u64);
                let val_str = format_runtime_value(val_raw, field_ty);
                parts.push(format!("{}={}", name, val_str));
            }
            format!("{{{}}}", parts.join(", "))
        }
        MonoType::Tuple(elems) => {
            if elems.is_empty() {
                return "()".to_string();
            }
            let ptr = raw as *const u8;
            if ptr.is_null() {
                return "()".to_string();
            }
            let mut parts = Vec::new();
            for (i, elem_ty) in elems.iter().enumerate() {
                let slot = ptr.add(i * 8);
                let val_raw = *(slot as *const u64);
                parts.push(format_runtime_value(val_raw, elem_ty));
            }
            format!("({})", parts.join(", "))
        }
        MonoType::Variant(cases, _) => {
            let ptr = raw as *const u8;
            if ptr.is_null() {
                return "||".to_string();
            }
            let tag = *(ptr as *const u64) as usize;
            if let Some((name, payload_ty)) = cases.get(tag).or_else(|| cases.first()) {
                if matches!(payload_ty.chase(), MonoType::Prim(Prim::Unit)) {
                    format!("|{}|", name)
                } else {
                    let payload_raw = *(ptr.add(8) as *const u64);
                    let payload_str = format_runtime_value(payload_raw, payload_ty);
                    format!("|{}={}|", name, payload_str)
                }
            } else {
                format!("|<variant {}>|", tag)
            }
        }
        MonoType::Array(_elem_ty) => {
            let ptr = raw as *const u8;
            if ptr.is_null() {
                return "[]".to_string();
            }
            "[]".to_string()
        }
        MonoType::FixedArray(elem_ty, len) => {
            let ptr = raw as *const u8;
            if ptr.is_null() || *len == 0 {
                return "[]".to_string();
            }
            let mut parts = Vec::new();
            for i in 0..*len {
                let slot = ptr.add(i * 8);
                let val_raw = *(slot as *const u64);
                parts.push(format_runtime_value(val_raw, elem_ty));
            }
            format!("[{}]", parts.join(", "))
        }
        MonoType::Fn(_, _) => "<closure>".to_string(),
        MonoType::Constraint(_, _, inner) => format_runtime_value(raw, inner),
        _ => (raw as i64).to_string(),
    }
}
