
// 漏洞样本：ReDoS——用户自定义正则 + 超长输入
// 漏洞点：允许用户输入任意正则表达式并匹配超长字符串，可触发灾难性回溯。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/regex-dos")
public class RegexDosBxB {

    


    @GetMapping("/unby/user-provided-pattern")
    public boolean unbyRegexWithUserPattern(
            @RequestParam String userProvidedRegex,
            @RequestParam String inputString) {
        // 危险实践：用户正则 + 用户超长输入，存在 ReDoS
        /*ANCHOR_1*/
        return inputString.matches(userProvidedRegex);
    }
}
