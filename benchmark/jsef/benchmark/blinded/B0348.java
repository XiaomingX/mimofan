package blinded;

/*
 * JSEF-Benchmark L3 — XStream allowlist 顺序修复（CWE-502）
 *
 * 修复：先设置 allowTypes 白名单，再执行 fromXML，确保解析受白名单约束。
 *
 * CWE-502 (Deserialization of Untrusted Data)。
 */
public class XstreamLateAllowlistBy {

    static Object fromXml(String xml) {
        System.out.println("[xstream-fromxml] " + xml);
        return null;
    }

    static void allowTypes(Class<?>... types) { /* 设置白名单 */ }

    




    public Object parse(String xml) {
        allowTypes(ByDto.class); // 先设白名单
        /*ANCHOR_1*/
        return fromXml(xml); // 解析时白名单已生效
    }

    static class ByDto {}

    public static void main(String[] args) {
        new XstreamLateAllowlistBy().parse("<ByDto/>");
    }
}
