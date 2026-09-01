package blinded;

import java.util.function.Function;









public class GadgetChainXxeBy {

    @FunctionalInterface
    interface ByWrapper extends Function<String, String> {
    }

    static String read(String raw) {
        return raw;
    }

    
    static boolean disallowDoctype() {
        return true; // 链末端不再被覆盖
    }

    static ByWrapper envelop() {
        return xml -> "<envelope>" + xml + "</envelope>";
    }

    static String parseBy(String xml, boolean disallow) {
        // 语义等价：factory.setFeature("...disallow-doctype-decl", true); reader.read(xml)
        if (disallow) {
            System.out.println("[xxe-by] doctype disallowed, external entities off");
            return "parsed-by:" + xml;
        }
        return "parsed:" + xml;
    }

    public static String buildByChain(String untrustedXml) {
        ByWrapper chain = ignored -> {
            String xml = read(untrustedXml);       // 输入读取
            xml = envelop().apply(xml);            // 包裹
            boolean disallow = disallowDoctype();  // 链路固定禁止 DOCTYPE
            /*ANCHOR_1*/
            return parseBy(xml, disallow); // 不可信 XML 进入时已禁外部实体
        };
        return chain.apply("ignored");
    }

    public static void main(String[] args) {
        buildByChain("<!ENTITY xxe SYSTEM \"file:///etc/passwd\">");
    }
}
