package blinded;

import java.net.URL;
import java.net.URLClassLoader;
import java.security.CodeSigner;






public class UnsignedJarLoadBy {

    


    public static void loadTrustedPlugin(String trustedPath) throws Exception {
        // source：受信的本地已签名 jar 路径
        URL url = new URL("file://" + trustedPath);
        try (URLClassLoader cl = new URLClassLoader(new URL[]{ url })) {
            Class<?> plugin = cl.loadClass("com.trusted.Plugin");
            /*ANCHOR_1*/
            CodeSigner[] signers = plugin.getProtectionDomain().getCodeSource().getCodeSigners();
            if (signers == null || signers.length == 0) {
                throw new SecurityException("插件未签名，拒绝加载");
            }
            plugin.getDeclaredConstructor().newInstance();
        }
    }
}
