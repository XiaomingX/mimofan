package blinded;

import java.io.ByteArrayInputStream;
import java.io.InputStream;

/*
 * JSEF-Benchmark L2 — Hessian 反序列化修复（CWE-502）
 *
 * 修复：对来源做校验 / 仅接受可信来源，并在 readObject 前校验期望类型。
 *
 * CWE-502 (Deserialization of Untrusted Data)。
 */
public class HessianBxBy {

    static final java.util.Set<String> ALLOWED = java.util.Set.of("com.jsef.dto.ByDto");

    




    public Object read(byte[] hessianBytes, boolean trustedSource) throws Exception {
        if (!trustedSource) {
            throw new SecurityException("untrusted source rejected");
        }
        InputStream is = new ByteArrayInputStream(hessianBytes);
        com.caucho.hessian.io.Hessian2Input in = new com.caucho.hessian.io.Hessian2Input(is);
        Object obj = in.readObject();
        if (obj != null && !ALLOWED.contains(obj.getClass().getName())) {
            throw new SecurityException("type not allowed");
        }
        /*ANCHOR_1*/
        return obj; // 仅可信来源 + 白名单类型
    }

    public static void main(String[] args) throws Exception {
        new HessianBxBy().read(new byte[]{0x00}, true);
    }
}
