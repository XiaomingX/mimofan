
// 安全对照：脚本引擎注入（修复版）
// 修复原则：避免执行用户提供的任何脚本内容；若必须使用，采用严格沙箱机制并禁用危险类。
//          本示例拒绝执行用户提供的脚本，移除 ScriptEngine.eval / CompiledScript.eval sink。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/script-engine")
public class ScriptEngineInjectionBy {

    


    @GetMapping("/by/engine-manager-exec")
    public String byScriptExecutionByManager(@RequestParam String userControlledScript) {
        // 安全实践：用户输入不再流向 ScriptEngine.eval。
        /*ANCHOR_1*/
        return "{\"status\":\"已拒绝执行不可信脚本（安全）\"}";
    }

    


    @GetMapping("/by/compiled-exec")
    public String byCompiledScriptExecution(@RequestParam String userControlledScript) {
        // 安全实践：用户输入不再流向 CompiledScript.eval。
        /*ANCHOR_2*/
        return "{\"status\":\"已拒绝执行不可信脚本（安全）\"}";
    }
}
