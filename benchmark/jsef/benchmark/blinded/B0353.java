package blinded;

import java.io.IOException;













public class JulietStyle_CommandInjection_CrossFile {

    


    public void bad(String userData) throws IOException {
        /*ANCHOR_1*/
        String command = "ls " + userData;
        Runtime.getRuntime().exec(command);
    }
}
