package blinded;

/*
 * JSEF-Benchmark L3 — XStream allowlist 顺序错误（CWE-502）
 *
 * 难度：L3（跨方法 / 顺序依赖）。正确做法须先设 allowTypes 再 fromXML；
 * 此处先 fromXML 解析（此时仍按默认无限制映射），再设置 allowTypes，
 * 导致 allowlist 对已解析对象无效，攻击载荷已实例化。
 *
 * CWE-502 (Deserialization of Untrusted Data)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用 XML。
 *
 * 修复要点（对照 XstreamLateAllowlistBy.java）：先 allowTypes 再 fromXML。
 */
public class XstreamLateAllowlistBx {

    // 抽象 sink：语义等价 com.thoughtworks.xstream.XStream.fromXML(xml)
    static Object fromXml(String xml) {
        System.out.println("[xstream-fromxml] " + xml);
        return null;
    }

    static void allowTypes(Class<?>... types) { /* 设置白名单 */ }

    




    public Object parse(String xml) {
        /*ANCHOR_1*/
        Object obj = fromXml(xml); // 先解析：此时无白名单保护
        allowTypes(ByDto.class); // 后设白名单：对已解析对象无效
        return obj;
    }

    static class ByDto {}

    public static void main(String[] args) {
        new XstreamLateAllowlistBx().parse("<map><entry>x</entry></map>");
    }
}
