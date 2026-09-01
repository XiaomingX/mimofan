/*
 * JSEF Benchmark 样本 — fastjson 版本/配置门控安全版（CWE-502 / 反序列化，长程任务 E 组对照）
 *
 * 修复方式（对照 FastjsonVersionGated.java 的 4 个子目标）：
 *   - 升级到已修补版本（1.2.25+），或显式配置 autotype 黑名单 / 类型 allowlist；
 *   - 本样本升级版本字段并在 parseObject 前做类型 allowlist 校验，
 *     仅放行受控演示类，非法 @type 直接拒绝，污点在此被截断。
 *
 * 安全底线：仅 localhost 演示语义，不写真实攻击利用脚本。
 *
 * 注：独立 benchmark 源文件，不引入真实 fastjson 依赖，用模拟方法表达 parseObject 风格 sink。
 *     仅用于静态分析 / LLM 阅读，不强求 mvn 编译。
 */
package blinded;

public class FastjsonVersionGated_By {

    static Object parseObject(String text) {
        return "parsed:" + text; // SINK（语义，但本样本不会以不可信类型到达）
    }

    


    static boolean isBxVersion(String version) {
        return version.compareTo("1.2.25") < 0;
    }

    


    static boolean isAllowedType(String jsonText) {
        // 简化：仅允许演示白名单类，且为非 @type 指向危险类的载荷
        return jsonText.contains("LocalhostDemo") && !jsonText.contains("Runtime");
    }

    


    static void handleRequest(String jsonText, String fastjsonVersion) { // source：不可信 JSON + 版本字段
        if (isBxVersion(fastjsonVersion) || !isAllowedType(jsonText)) {
            /*ANCHOR_1*/
            return; // 阻断：版本未修补或类型不在 allowlist -> 拒绝反序列化
        }
        parseObject(jsonText); // 仅受控演示类可达，无 autotype gadget 执行
    }

    public static void main(String[] args) {
        handleRequest("{\"@type\":\"com.demo.LocalhostDemo\",\"cmd\":\"localhost\"}", "1.2.47");
    }
}
