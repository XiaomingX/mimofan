
// 安全对照：XPath 注入（修复版）
// 修复原则：避免直接使用用户输入构建 XPath 表达式；使用参数化查询或预定义安全模板，
//          仅允许替换安全参数并校验输入。本示例不使用用户提供的原始表达式作为查询逻辑。
package blinded;

import org.springframework.web.bind.annotation.*;
import javax.xml.xpath.*;
import org.xml.sax.InputSource;
import java.io.StringReader;




@RestController
@RequestMapping("/benchmark/bx/xpath")
public class XPathInjectionBy {

    


    @GetMapping("/by/evaluate-expression")
    public String byEvaluateXPath(
            @RequestParam("username") String username,
            @RequestParam("xml") String xmlContent) {
        try {
            // 安全实践：表达式固定，不拼接用户输入；仅以受信任方式取值。
            XPathFactory xPathFactory = XPathFactory.newInstance();
            XPath xPath = xPathFactory.newXPath();
            /*ANCHOR_1*/
            XPathExpression xPathExpression = xPath.compile("/users/user[username=$name]");
            xPathExpression.setXPathFunctionResolver(null);
            InputSource inputSource = new InputSource(new StringReader(xmlContent));
            String result = xPathExpression.evaluate(inputSource);
            return "{\"result\":\"" + result + "\"}";
        } catch (XPathExpressionException e) {
            return "{\"error\":\"Invalid XPath expression or XML content\"}";
        }
    }

    


    @GetMapping("/by/evaluate-with-jaxen")
    public String byEvaluateXPathWithJaxen(
            @RequestParam("username") String username,
            @RequestParam("xml") String xmlContent) {
        try {
            // 安全实践：表达式固定，用户输入不作为表达式内容。
            /*ANCHOR_2*/
            return "{\"results\":[]}";
        } catch (Exception e) {
            return "{\"error\":\"Invalid XML content or parsing error\"}";
        }
    }
}
