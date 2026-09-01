package blinded;

import javax.naming.directory.DirContext;
import javax.naming.directory.InitialDirContext;
import java.util.Hashtable;










public class L0LdapDirect {

    




    public void run(Hashtable<String, Object> env, String userInput) throws Exception {
        DirContext ctx = new InitialDirContext(env);
        /*ANCHOR_1*/
        ctx.search("ou=people", "(uid=" + userInput + ")", null, null);
    }

    public static void main(String[] args) {
        System.out.println("demo: search (uid=localhost-demo)");
    }
}
