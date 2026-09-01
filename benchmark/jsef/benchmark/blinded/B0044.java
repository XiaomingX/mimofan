
// 安全对照：BeanShell 脚本注入（修复版）
// 修复原则：严禁执行用户提供的任何脚本代码。若必须执行动态逻辑，使用受信任的固定脚本或
//          预定义白名单，绝不允许用户输入作为可执行脚本内容。
package blinded;

import org.springframework.web.bind.annotation.*;





@RestController
@RequestMapping("/benchmark/bx/beanshell")
public class BeanshellInjectionBy {

    


    @GetMapping("/by/evaluate-beanshell")
    public String byEvaluateBeanshellScript(@RequestParam String userProvidedScript) {
        // 安全实践：不将用户输入交给任何脚本引擎；此处仅做受信任处理。
        /*ANCHOR_1*/
        if (userProvidedScript == null || userProvidedScript.isEmpty()) {
            return "{\"message\": \"缺少脚本输入\"}";
        }
        return "{\"message\": \"已拒绝执行不可信脚本（安全）\"}";
    }

    


    @GetMapping("/by/execute-system-command")
    public String byExecuteSystemCommand(@RequestParam String userCommand) {
        // 安全实践：用户输入的命令永不被执行。
        /*ANCHOR_2*/
        return "{\"message\": \"已拒绝执行不可信命令（安全）\"}";
    }
}
