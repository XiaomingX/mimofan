
// 安全对照：OGNL 表达式注入（修复版）
// 修复原则：严禁使用用户输入直接拼接或作为 OGNL 表达式执行；采用严格白名单与沙箱。
//          本示例拒绝执行用户提供的 OGNL 表达式。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/ognl")
public class OgnlInjectionBy {

    


    @GetMapping("/by/evaluate-raw")
    public String byEvaluateRawOgnl(@RequestParam String userControlledOgnlExpression) {
        // 安全实践：用户输入不再流向 Ognl.getValue/parseExpression。
        /*ANCHOR_1*/
        return "{\"status\":\"已拒绝执行不可信 OGNL 表达式（安全）\"}";
    }

    


    @GetMapping("/by/exec-command")
    public String byExecuteSystemCommand(@RequestParam String userInputCommand) {
        // 安全实践：用户输入不再拼接为 Ognl.getValue(Runtime.exec) 表达式。
        /*ANCHOR_2*/
        return "{\"status\":\"已拒绝构造不可信命令表达式（安全）\"}";
    }
}
