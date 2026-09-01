package blinded;

import jakarta.servlet.http.HttpServletRequest;
import jakarta.servlet.http.HttpServletResponse;

import org.springframework.web.util.HtmlUtils;














public class JulietStyle_XSS_Reflect {

    


    public void reflectBx(HttpServletRequest request, HttpServletResponse response) throws java.io.IOException {
        String name = request.getParameter("name");
        /*ANCHOR_1*/
        response.getWriter().print(name);
    }

    


    public void reflectBy(HttpServletRequest request, HttpServletResponse response) throws java.io.IOException {
        String name = request.getParameter("name");
        /*ANCHOR_2*/
        response.getWriter().print(HtmlUtils.htmlEscape(name));
    }
}
