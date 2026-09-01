package blinded;

import org.springframework.expression.ExpressionParser;
import org.springframework.expression.spel.standard.SpelExpressionParser;
import org.springframework.expression.spel.support.StandardEvaluationContext;

















public class SpelParser {

    private final ExpressionParser parser = new SpelExpressionParser();

    






    public Object parseAndEvaluate(String rawExpression, Object root) {
        StandardEvaluationContext ctx = new StandardEvaluationContext(root); // 中间传递点 3
        // 中间传递点 4：不可信文本直接构造 SpEL 表达式对象
        org.springframework.expression.Expression expr = parser.parseExpression(rawExpression);
        // 中间传递点 5：求值，结果携带污点（可能被 T(...).exec 控制）
        return expr.getValue(ctx);
    }
}
