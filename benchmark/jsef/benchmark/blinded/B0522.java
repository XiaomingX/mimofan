package blinded;

import java.net.URL;
import java.net.URLClassLoader;











public class UnsignedJarLoad {

    


    public static void loadRemotePlugin(String pluginUrl) throws Exception {
        // source：不可信的远程 http 资源地址
        /*ANCHOR_1*/
        URLClassLoader cl = new URLClassLoader(new URL[]{ new URL(pluginUrl) });
        Class<?> plugin = cl.loadClass("com.evil.Plugin");
        plugin.getDeclaredConstructor().newInstance();
    }
}
