/** Official Minecraft EULA snapshot for in-app acceptance.
 * Source of truth: https://www.minecraft.net/en-us/eula (aka.ms/MinecraftEULA)
 * Mojang/Microsoft may update the live document; always prefer the official URL.
 */

export const EULA_OFFICIAL_URL = 'https://www.minecraft.net/en-us/eula'
export const EULA_AKA_URL = 'https://aka.ms/MinecraftEULA'
export const EULA_MSA_URL = 'https://www.microsoft.com/servicesagreement'
export const EULA_USAGE_URL = 'https://www.minecraft.net/en-us/usage-guidelines'

export type EulaSection = { title: string; body: string }

export const EULA_SECTIONS: EulaSection[] = [
  {
    title: 'Minecraft End(er)-User License Agreement (“EULA”)',
    body: `This EULA is a legal agreement between you and us (Mojang AB and Microsoft Corporation, or, if applicable, one of its local affiliates listed in the Company Information section below). You should read the whole thing but here is a quick summary of some important points to help guide you - the full terms and conditions still apply though.

• This Minecraft EULA and the Microsoft Services Agreement, together, apply to all Minecraft services.
• Your content is yours, but please share it responsibly and safely.
• Our community standards help us build a community that is open and safe for everyone.
• You may develop tools, plug-ins and services as long as they do not seem official or approved by us, such as by using our logos.
• Do not distribute or make commercial use of anything we've made without our permission.
• We are trying to be open, honest and trusting with the hope that you hold us in the same regard.

This EULA applies to all Minecraft websites, software, experiences, and services (“Services”), except for the Minecraft Shop and Minecraft Education, each of which have their own separate terms.`,
  },
  {
    title: 'Introduction',
    body: `If you buy, download, or use any of our Services, or if you click to accept this EULA, that means you agree to this Minecraft EULA and the Microsoft Services Agreement, so please read through them carefully. If you are a minor and you are having trouble understanding these terms and conditions, please ask your parent or legal guardian to explain them, especially as your parent or legal guardian is responsible for the creation of your Microsoft account and the acceptance all terms on your behalf. Remember to check here and the Microsoft Services Agreement once in a while as we may update these terms and conditions, which will be effective the next time you use our Services.`,
  },
  {
    title: 'ACCOUNT Terms',
    body: `For the Microsoft platforms (including our website, Microsoft Store and Xbox), we use Microsoft accounts for our games and a Microsoft account is required to purchase our games or a Minecraft Realms subscription through our website, the Microsoft Store, or Xbox. The Microsoft Services Agreement has all the terms that apply to your Microsoft account.

If you purchased our games through a platform that does not require a Microsoft account, such as Sony PlayStation, Nintendo, Apple iOS, Google Play, or Steam, please view those platform’s terms as those will apply to your purchase. The Microsoft Services Agreement may still apply to the extent you use a Microsoft account in connection with our Services (such as cross-platform play and Minecraft Realms).

An exception here is Minecraft Education. Minecraft Education is provided through the group agreement in place with the school or organization that purchased Minecraft Education for your use, so please view your group’s terms for your legal agreement.

Another exception is the Minecraft Shop. Minecraft Shop is managed by our friends at Snow Commerce, who are not part of us or Microsoft. Please review their terms and conditions, as they apply to your Minecraft Shop purchases.

If you originally signed up for a Mojang Account in the past, you must migrate to a Microsoft Account in order to keep using the Services.`,
  },
  {
    title: 'What you can and can’t do with Minecraft software and content',
    body: `When you buy our games, that means you can download, install, and play them. For the server version of Minecraft: Java Edition, you can install it on a server and host online play.

However, you must not distribute anything we've made unless we specifically agree to it. By "distribute anything we've made" what we mean is:

• give copies of our game software or content to anyone else;
• make commercial use of anything we've made;
• try to make money from anything we've made; or
• let other people get access to anything we've made in a way that is unfair or unreasonable.

And so that we are crystal clear, "the game" or "what we have made" includes, but is not limited to, the Services, plus any other games we might publish in the future. It also includes updates, patches, downloadable or Marketplace content, add-ons, or modified versions of a game, part of those things, merchandise, audio-visual content, or anything else we've made.

Otherwise we are quite relaxed about what you do - in fact we really encourage you to do cool stuff - but just don't do those things that we say you can't. We've put together detailed Minecraft Usage Guidelines as to how you can or cannot do things using what we've made, including screenshots and recorded videos of our games. These Minecraft Usage Guidelines are extra permissions that we give to the community to encourage creativity and community, but we reserve the right to change them or withdraw permissions, especially if we see people exploiting or abusing these permissions.`,
  },
  {
    title: 'USING mods',
    body: `If you've bought Minecraft: Java Edition, you may play around with it and modify it by adding modifications, tools, or plugins, which we will refer to collectively as "Mods." By "Mods," we mean something original that you or someone else created that doesn't contain a substantial part of our copyrightable code or content. When you combine your Mod with Minecraft: Java Edition, we will call that combination a "Modded Version" of the game. We have the final say on what constitutes a Mod and what doesn't. You may not distribute any Modded Versions of our game or software, and we'd appreciate it if you didn't use Mods for griefing. Basically, Mods are okay to distribute; hacked versions or Modded Versions of the game client or server software are not okay to distribute.

Any Mods you create for Minecraft: Java Edition from scratch belong to you (including pre-run Mods and in-memory Mods) and you can do whatever you want with them, as long as you don't sell them for money / try to make money from them and so long as you don't distribute Modded Versions of the game. Remember that a Mod means something that is your original work and that does not contain a substantial part of our code or content. You only own what you created; you do not own our code or content.

When we update our games, some changes might not work well with other software, such as Mods. This is unfortunate, but it is something we don't take responsibility for. If that is the case, try running an older version.

In order to ensure the integrity of our games, we need all game downloads and updates to come from a source that we authorize. It's also important for us that 3rd party tools/services don't seem "official" as we can't guarantee their quality.`,
  },
  {
    title: 'CONTENT',
    body: `The Microsoft Services Agreement says “Your Content remains Your Content”, and that applies to Minecraft. We don't own the original stuff that you create. We will however own things that are copies (or substantial copies) or derivatives of our property and creations - but if you create original things, they aren't ours. So, as an example:

• a single Minecraft block (including its textures and its “look and feel”) - we own that;
• your creation of a Gothic Cathedral with a rollercoaster running through it - we don't own that.`,
  },
  {
    title: 'ONLINE SAFETY',
    body: `Please watch out if you are talking to people in our games. It is hard for either you or us to know for sure that what people say is true, or even if people are really who they say they are. You should think twice about giving out information about yourself.

We have helpful resources that can help you be safe on the Internet.`,
  },
  {
    title: 'COMMUNITY STANDARDS FOR MINECRAFT',
    body: `As an Xbox Game Studio, Mojang Studios affirms the Xbox Community Standards and all Minecraft players held responsible to those standards to participate in the Minecraft community. These Community Standards for Minecraft is a supplement to the Xbox Community Standards and is a statement of our values to keep the Minecraft community safe and fun for everyone.

Our Values
1. Minecraft is for everyone
2. Diversity powers our community
3. Playing with others should be safe and inclusive
4. Hate has no place here

To keep the Minecraft community welcoming and inclusive for everyone, we have a zero-tolerance policy towards hate speech, terrorist or violent extremist content, bullying, harassing, sexual solicitation, fraud, or threatening others.

We reserve the right to suspend or permanently ban anyone who violates these Community Standards or this EULA.`,
  },
  {
    title: 'REALMS',
    body: `Minecraft Realms (“Realms”) is our online service that allows people to play with others on dedicated servers that are hosted by us. Realms is not included with your purchase of Minecraft; it is an add-on to the game.

When you get your Realm you will get access to a dedicated Realm, on which you can play Minecraft by yourself or you can invite several other people to play Minecraft with you. However you cannot do the following:

• sell, lease, rent, transfer, give away, or otherwise deal in access to your Realm or receive financial, commercial or other benefits for letting other people play on your Realm.

When you pay for the use of Realms, you are not buying ownership of the physical server hardware supporting your Realm – you are buying a permission to use Realms in accordance with this EULA.`,
  },
  {
    title: 'privacy',
    body: `The Microsoft Privacy Statement applies to all Minecraft Services. The sole exception is the Minecraft Shop, which is managed by our friends at Snow Commerce, who are not part of us or Microsoft.`,
  },
  {
    title: 'GENERAL STUFF',
    body: `Your local law may give you rights that this EULA cannot change; if so, this EULA applies as far as the law allows.

We may change this EULA from time to time, if we have reason to, such as changes to our games, our practices, or our legal obligation. But those changes will be effective only to the extent that they can legally apply. In that case we'll inform you of the change before it takes effect, either by posting a notice on our website or by other reasonable means.

If you come to us with a suggestion for Minecraft, that suggestion is made for free and we have no obligation to accept or consider it. This means we can use or not use your suggestion in any way we want and we don't have to pay you for it.`,
  },
  {
    title: 'COMPANY INFORMATION',
    body: `Mojang AB
Söder Mälarstrand 43
SE-11825, Stockholm
Sweden
Organization number: 556819-2388

Microsoft Corporation
One Microsoft Way
Redmond, WA 98052-6399, U.S.A

For users in Brazil:
Microsoft do Brasil Importação e Comércio de Software e Video Games Ltda.
Avenida Nações Unidas, 12901
City of Sao Paulo, State of Sao Paulo, 04578-000, Brazil`,
  },
]

export const EULA_ZH_SUMMARY = `运行 Minecraft: Java Edition 服务端前，你必须同意 Mojang / Microsoft 的最终用户许可协议（EULA）。

要点（非正式翻译，以英文原文为准）：
• 你可以安装并托管官方服务器端供在线游玩。
• 不得擅自分发、商业化 Mojang/Microsoft 制作的游戏内容。
• 可为 Java 版制作 Mods/插件，但不得分发被修改过的完整客户端/服务端。
• 你创建的原创内容归你所有；方块贴图等游戏资产仍属 Mojang/Microsoft。
• 社区标准要求包容、禁止仇恨与骚扰等行为。
• 完整条款以官网为准，可能随时更新。`
