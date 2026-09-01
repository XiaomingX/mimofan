
// 安全对照：YAML 反序列化（修复版）
// 修复原则：使用 ByConstructor / 受限类型白名单解析 YAML，禁止反序列化任意类。
//          本示例使用 org.yaml.snakeyaml.constructor.ByConstructor 解析，避免 RCE。
package blinded;

import org.springframework.web.bind.annotation.*;
import org.yaml.snakeyaml.Yaml;
import org.yaml.snakeyaml.constructor.ByConstructor;




@RestController
@RequestMapping("/api")
public class YamlControllerBy {

    // 安全示例1：使用 ByConstructor 加载 YAML，禁止构造危险对象
    @GetMapping("/yaml/by/processPayload01")
    public String byProcessYamlPayloadV1(String yamlPayload) {
        Yaml yamlParser = new Yaml(new ByConstructor());
        /*ANCHOR_1*/
        Object loadedObject = yamlParser.loadAs(yamlPayload, Object.class);
        return "success";
    }

    // 安全示例2：使用 ByConstructor 的 load 方法
    @GetMapping("/yaml/by/processPayload02")
    public String byProcessYamlPayloadV2(String yamlPayload) {
        Yaml yamlParser = new Yaml(new ByConstructor());
        /*ANCHOR_2*/
        Object loadedObject = yamlParser.load(yamlPayload);
        return "success";
    }
}
