
// 安全对照：Groovy 代码注入（修复版）
// 修复原则：避免执行任何来自不可信来源的脚本代码；如必须使用，实施严格的输入验证、
//          白名单机制与最小权限沙箱。本示例直接拒绝执行不可信脚本。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/groovy")
public class GroovyInjectionBy {

    


    @GetMapping("/by/evaluator")
    public String byScriptWithEvaluator(@RequestParam String groovyScript) {
        // 安全实践：用户输入不再流向 GroovyScriptEvaluator.evaluate。
        /*ANCHOR_1*/
        return "{\"message\":\"已拒绝执行不可信 Groovy 脚本（安全）\"}";
    }

    


    @GetMapping("/by/shell")
    public String byScriptWithShell(@RequestParam String groovyScript) {
        // 安全实践：用户输入不再流向 GroovyShell.evaluate。
        /*ANCHOR_2*/
        return "{\"message\":\"已拒绝执行不可信 Groovy 脚本（安全）\"}";
    }
}
