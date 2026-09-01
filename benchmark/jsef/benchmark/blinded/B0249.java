
// 安全对照：ReDoS——用户自定义正则（修复版）
// 修复原则：限制输入长度、禁用危险构造；使用简单白名单正则替代用户输入。
package blinded;

import org.springframework.web.bind.annotation.*;
import java.util.regex.Pattern;




@RestController
@RequestMapping("/benchmark/bx/regex-dos")
public class RegexDosByB {

    


    @GetMapping("/by/user-provided-pattern")
    public boolean byRegexWithUserPattern(
            @RequestParam String userProvidedRegex,
            @RequestParam String inputString) {
        if (inputString == null || inputString.length() > 100) {
            return false;
        }
        // 安全实践：不使用用户提供的正则，避免 ReDoS
        /*ANCHOR_1*/
        Pattern pattern = Pattern.compile("[A-Za-z0-9 ]+");
        return pattern.matcher(inputString).matches();
    }
}
