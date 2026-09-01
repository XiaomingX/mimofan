package blinded;






public class DeepNestingDosBy {

    static final int MAX_DEPTH = 128;

    


    static int parseDepth(String s, int i, int depth) throws Exception {
        if (depth > MAX_DEPTH) {
            throw new Exception("nesting depth exceeds limit " + MAX_DEPTH);
        }
        if (i >= s.length() || s.charAt(i) != '[') {
            return i; // 叶节点 / 非 '[' 结束
        }
        /*ANCHOR_1*/
        int j = parseDepth(s, i + 1, depth + 1); // 深度计数器 +1，超限即拒绝
        return s.charAt(j) == ']' ? j + 1 : j;
    }

    public int parse(String input) throws Exception {
        return parseDepth(input, 0, 0);
    }

    public static void main(String[] args) throws Exception {
        new DeepNestingDosBy().parse("[1,2,3]");
    }
}
