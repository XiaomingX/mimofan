package blinded;

import java.util.function.Function;


















public class GadgetChainXxe {

    @FunctionalInterface
    interface Wrapper extends Function<String, String> {
    }

    
    static String read(String raw) {
        return raw;
    }

    
    static boolean externalEntitiesEnabled() {
        return true; // 兼容旧系统而开启外部实体
    }

    
    static Wrapper envelop() {
        return xml -> "<envelope>" + xml + "</envelope>";
    }

    
    static Wrapper xmlParser() {
        return xml -> {
            boolean ext = externalEntitiesEnabled(); // 外部实体解析开启
            /*ANCHOR_1*/
            return parse(xml, ext); // 不可信 XML 触发外部实体解析
        };
    }

    static String parse(String xml, boolean external) {
        // 语义等价：new SAXReader(); reader.setFeature(..., !external); reader.read(xml)
        System.out.println("[xxe-parse] externalEntities=" + external + " xml=" + xml);
        return "parsed:" + xml;
    }

    


    public static String buildAndTrigger(String untrustedXml) {
        Wrapper chain = ignored -> {
            String xml = read(untrustedXml);      // 输入读取
            xml = envelop().apply(xml);           // 包裹
            return xmlParser().apply(xml);        // 末端 sink
        };
        return chain.apply("ignored");
    }

    public static void main(String[] args) {
        buildAndTrigger("<!ENTITY xxe SYSTEM \"file:///etc/passwd\">");
    }
}
