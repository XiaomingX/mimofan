package blinded;











public class XssJsContextMismatchBy {

    
    static String escapeJsString(String value) {
        StringBuilder sb = new StringBuilder();
        for (int i = 0; i < value.length(); i++) {
            char c = value.charAt(i);
            switch (c) {
                case '\\': sb.append("\\\\"); break;
                case '\'': sb.append("\\'"); break;
                case '"':  sb.append("\\\""); break;
                case '\n': sb.append("\\n"); break;
                case '\r': sb.append("\\r"); break;
                case '<':  sb.append("\\u003c"); break;
                case '>':  sb.append("\\u003e"); break;
                default:   sb.append(c);
            }
        }
        return sb.toString();
    }

    




    public String render(String user) {
        String js = "var name = '" + escapeJsString(user) + "';"; // JS 上下文专用转义
        /*ANCHOR_1*/
        return "<script>" + js + "</script>";
    }
}
