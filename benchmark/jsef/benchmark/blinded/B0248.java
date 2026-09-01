
// 安全对照：正则表达式 DOS / ReDoS（修复版）
// 修复原则：限制输入长度、使用简单正则、设置匹配超时，避免灾难性回溯。
package blinded;

import org.springframework.web.bind.annotation.*;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;




@RestController
@RequestMapping("/benchmark/bx/regex-dos")
public class RegexDosBy {

    


    @GetMapping("/by/nested-repetition")
    public boolean byRegexWithNestedRepetition(@RequestParam String inputString) {
        // 防护1：限制输入长度
        if (inputString == null || inputString.length() > 100) {
            return false;
        }
        try {
            // 防护2：使用简单正则，无 (a+)+b 式嵌套重复
            /*ANCHOR_1*/
            Pattern pattern = Pattern.compile("[a-zA-Z0-9]+", Pattern.CASE_INSENSITIVE);
            return pattern.matcher(inputString).matches();
        } catch (PatternSyntaxException e) {
            return false;
        }
    }
}
