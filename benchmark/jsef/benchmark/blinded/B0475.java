
package blinded;












public class DeepNestingDosBx {

    


    static int parseDepth(String s, int i) {
        if (i >= s.length() || s.charAt(i) != '[') {
            return i; // 叶节点 / 非 '[' 结束
        }
        /*ANCHOR_1*/
        int j = parseDepth(s, i + 1); // 每层 '[' 无限递归，深度无上限 → 栈溢出
        return s.charAt(j) == ']' ? j + 1 : j;
    }

    public int parse(String input) {
        return parseDepth(input, 0);
    }

    public static void main(String[] args) {
        // 演示语义：仅说明嵌套深度 > 栈深时触发 StackOverflowError，不提供真实载荷
        new DeepNestingDosBx().parse("[".repeat(20000) + "]".repeat(20000));
    }
}
