package com.jsef.benchmark.sec.jep290dead;

import java.io.ByteArrayInputStream;
import java.io.IOException;
import java.io.ObjectInputFilter;
import java.io.ObjectInputStream;

/**
 * JSEF-Benchmark L3 — JEP290 白名单过滤器修复（CWE-502 SAFE）
 *
 * 修复：真实白名单 filter —— 危险包直接 REJECTED，白名单内 ALLOWED，
 * 其余一律 REJECTED。readObject 仅对白名单类型放行。
 *
 * CWE-502 (Deserialization of Untrusted Data)。
 */
public class Jep290DeadFilterSafe {

    /** 真实白名单：危险包拒绝，白名单内放行，其余一律拒绝 */
    private static final ObjectInputFilter WHITELIST = info -> {
        Class<?> clazz = info.serialClass();
        if (clazz == null) {
            return ObjectInputFilter.Status.UNDECIDED; // 数组/基本类型无关紧要
        }
        String name = clazz.getName();
        if (name.startsWith("java.util.") || name.equals("com.jsef.benchmark.sec.jep290dead.SafeDto")) {
            return ObjectInputFilter.Status.ALLOWED;
        }
        return ObjectInputFilter.Status.REJECTED; // 危险包一律拒绝
    };

    public Object read(byte[] payload) throws IOException, ClassNotFoundException {
        ObjectInputStream ois = new ObjectInputStream(new ByteArrayInputStream(payload));
        ois.setObjectInputFilter(WHITELIST);
        // [VULN] 安全：readObject 仅对白名单类型放行
        // [CHECKPOINT id=JSEF-JEP290-001S cwe=502 level=L3 source=serialized payload sink=whitelist ObjectInputFilter rejects then readObject expect=SAFE]
        return ois.readObject();
    }

    static class SafeDto {}
}
