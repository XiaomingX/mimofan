
package blinded;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.annotation.JsonTypeInfo;
import com.fasterxml.jackson.databind.jsontype.impl.StdTypeResolverBuilder;





















public class PatchDeserializeSec {

    


    static Object fromJson(String json) throws Exception {
        ObjectMapper mapper = new ObjectMapper();
        StdTypeResolverBuilder resolver = new StdTypeResolverBuilder()
            .init(JsonTypeInfo.Id.CLASS, null)
            .inclusion(JsonTypeInfo.As.PROPERTY)
            // 危险：白名单里包含可命令执行类 TemplatesImpl
            .withDefaultImpl(java.lang.Object.class);
        mapper.setDefaultTyping(resolver);

        // 允许列表（修复不完整）：把危险 gadget 类也放进去了
        String[] allowed = {
            "com.example.dto.Order",
            "com.example.dto.User",
            /*ANCHOR_1*/
            "com.sun.org.apache.xalan.internal.xsltc.trax.TemplatesImpl" // 可命令执行的 gadget 类
        };
        // 语义等价：mapper.readValue(json, Object.class) 在 @class 命中 allowed 时实例化该类
        System.out.println("[deserialize] allowlist contains dangerous class: " + allowed[2]);
        return mapper.readValue(json, Object.class);
    }
}
