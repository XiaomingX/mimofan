
// 安全对照：MVEL 表达式注入（修复版）
// 修复原则：避免使用用户输入作为表达式直接执行；如必须计算表达式，使用受信任的常量
//          表达式并彻底隔离用户输入。本示例拒绝执行用户提供的表达式。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/mvel")
public class MvelInjectionBy {

    


    @GetMapping("/by/eval-direct")
    public String byMvelDirectEval(@RequestParam String userInputExpression) {
        // 安全实践：用户输入不再流向 MVEL.eval。
        /*ANCHOR_1*/
        return "{\"status\":\"已拒绝执行不可信表达式（安全）\"}";
    }

    


    @GetMapping("/by/eval-compiled")
    public String byMvelCompiledEval(@RequestParam String userInputExpression) {
        // 安全实践：用户输入不再流向 MVEL.executeExpression。
        /*ANCHOR_2*/
        return "{\"status\":\"已拒绝执行不可信表达式（安全）\"}";
    }
}
