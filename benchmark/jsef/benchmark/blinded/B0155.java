package blinded;

import javax.naming.directory.DirContext;
import javax.naming.directory.InitialDirContext;
import java.util.Hashtable;









public class L0LdapDirectBy {

    




    public void run(Hashtable<String, Object> env, String userInput) throws Exception {
        DirContext ctx = new InitialDirContext(env);
        String by = escapeLdap(userInput);
        /*ANCHOR_1*/
        ctx.search("ou=people", "(uid=" + by + ")", null, null);
    }

    private static String escapeLdap(String s) {
        StringBuilder sb = new StringBuilder();
        for (char c : s.toCharArray()) {
            if (c == '*' || c == '(' || c == ')' || c == '\\' || c == '\0') {
                sb.append('\\').append(c);
            } else {
                sb.append(c);
            }
        }
        return sb.toString();
    }

    public static void main(String[] args) throws Exception {
        new L0LdapDirectBy().run(new Hashtable<>(), "localhost-demo");
    }
}
