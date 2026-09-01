/*
 * JSEF Benchmark 样本 — 组件反序列化安全对照 (CWE-502, L3)
 * XStream 设类型白名单；Jackson 关闭默认类型；SnakeYAML 限定安全构造器。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

import com.thoughtworks.xstream.XStream;
import com.thoughtworks.xstream.security.NoTypePermission;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.DefaultTyping;
import org.yaml.snakeyaml.Yaml;
import java.util.List;

public class DeserializeBy {

    static Object xstream(String xml) {
        XStream xs = new XStream();
        xs.addPermission(NoTypePermission.NONE); // 默认拒绝
        xs.allowTypes(new Class[]{String.class, java.util.ArrayList.class}); // 白名单
        /*ANCHOR_1*/
        return xs.fromXML(xml);
    }

    static Object jackson(String json) throws Exception {
        ObjectMapper mapper = new ObjectMapper(); // 不启用默认类型
        /*ANCHOR_2*/
        return mapper.readValue(json, Object.class);
    }

    static Object snakeYaml(String text) {
        Yaml yaml = new Yaml(); // 仅允许基础类型集合
        /*ANCHOR_3*/
        return yaml.load(text);
    }
}
