// ignore: unused_import
import 'package:intl/intl.dart' as intl;

import 'app_localizations.dart';

// ignore_for_file: type=lint

/// The translations for Portuguese (`pt`).
class LPt extends L {
  LPt([String locale = 'pt']) : super(locale);

  @override
  String get appName => 'Enxame';

  @override
  String get onboardingTitle => 'Bem-vindo ao Enxame';

  @override
  String get onboardingSubtitle =>
      'Conversas e arquivos direto entre amigos, sem servidor no meio. Basta escolher um nome.';

  @override
  String get onboardingNameLabel => 'Como você quer ser chamado?';

  @override
  String get onboardingStart => 'Começar';

  @override
  String get onboardingStarting => 'Preparando seu nó…';

  @override
  String startFailed(String error) {
    return 'Não foi possível iniciar: $error';
  }

  @override
  String get navChats => 'Conversas';

  @override
  String get navContacts => 'Contatos';

  @override
  String get navContribution => 'Contribuição';

  @override
  String get navDiagnostics => 'Diagnóstico';

  @override
  String get noChats =>
      'Nenhuma conversa ainda. Adicione um contato para começar.';

  @override
  String get pickChat => 'Escolha uma conversa';

  @override
  String get newGroup => 'Novo grupo';

  @override
  String get groupNameLabel => 'Nome do grupo';

  @override
  String get groupMembersLabel => 'Membros';

  @override
  String get create => 'Criar';

  @override
  String get cancel => 'Cancelar';

  @override
  String get close => 'Fechar';

  @override
  String get ok => 'Entendi';

  @override
  String get groupNeedsContacts =>
      'Você precisa de pelo menos um contato para criar um grupo.';

  @override
  String get groupBadge => 'grupo';

  @override
  String get typeMessage => 'Escreva uma mensagem';

  @override
  String get send => 'Enviar';

  @override
  String sendFailed(String error) {
    return 'Falha ao enviar: $error';
  }

  @override
  String get deliveredHint => 'entregue';

  @override
  String get pendingHint => 'aguardando entrega';

  @override
  String get noMessages => 'Nenhuma mensagem ainda. Diga oi!';

  @override
  String get you => 'você';

  @override
  String get myInvite => 'Meu convite';

  @override
  String get myInviteHelp =>
      'Mande este convite para quem você quer como contato. Ele vale por algumas horas e a pessoa só entra se você aceitar.';

  @override
  String get copyInvite => 'Copiar convite';

  @override
  String get inviteCopied => 'Convite copiado';

  @override
  String get showQr => 'Mostrar QR';

  @override
  String get addContact => 'Adicionar contato';

  @override
  String get addContactHelp => 'Cole o convite que a outra pessoa mandou.';

  @override
  String get inviteLabel => 'Convite (enxame1…)';

  @override
  String get sendRequest => 'Enviar pedido';

  @override
  String get waitingAcceptance =>
      'Pedido enviado. Aguardando a outra pessoa aceitar…';

  @override
  String contactAdded(String name) {
    return '$name agora é seu contato.';
  }

  @override
  String get contactRejected => 'A outra pessoa recusou o pedido.';

  @override
  String get contactAlready => 'Essa pessoa já é seu contato.';

  @override
  String get requestsTitle => 'Pedidos de contato';

  @override
  String requestFrom(String name) {
    return '$name quer ser seu contato';
  }

  @override
  String get requestVerify =>
      'Confirme esta impressão digital com a pessoa por outro canal antes de aceitar:';

  @override
  String get accept => 'Aceitar';

  @override
  String get reject => 'Recusar';

  @override
  String get contactsTitle => 'Contatos';

  @override
  String get noContacts => 'Você ainda não tem contatos.';

  @override
  String get contactPending => 'aguardando';

  @override
  String get fingerprintLabel => 'Impressão digital';

  @override
  String get contributionTitle => 'Contribuição para a rede';

  @override
  String get contributionHelp =>
      'Seu computador ajuda seus contatos guardando mensagens e arquivos enquanto eles estão offline. Tudo cifrado: você não lê nada do que carrega.';

  @override
  String get roleLabel => 'Papel atual';

  @override
  String get gameMode => 'Modo jogo';

  @override
  String get gameModeHelp =>
      'Pausa toda a contribuição enquanto você joga ou usa muita CPU/rede.';

  @override
  String get contribute => 'Contribuir com a rede';

  @override
  String get contributeHelp =>
      'Desligado, você só usa a rede e não ajuda ninguém.';

  @override
  String get servedThisMonth => 'Servido neste mês';

  @override
  String get carrying => 'Guardando para contatos';

  @override
  String get itemsDelivered => 'Entregas feitas';

  @override
  String get byContact => 'Quem você ajudou';

  @override
  String get nobodyYet => 'Ninguém ainda.';

  @override
  String messagesUnit(int n) {
    return '$n mensagens';
  }

  @override
  String get diagnosticsTitle => 'Diagnóstico de rede';

  @override
  String get runDiagnostics => 'Executar diagnóstico';

  @override
  String get running => 'Medindo…';

  @override
  String get dnsLabel => 'DNS';

  @override
  String get natLabel => 'NAT';

  @override
  String get relayLabel => 'Relay preferido';

  @override
  String get pathDirect => 'direto';

  @override
  String get pathRelay => 'via relay';

  @override
  String get pathMixed => 'misto';

  @override
  String get pathUnknown => 'desconhecido';

  @override
  String get unreachable => 'inacessível';

  @override
  String get noContactsToProbe => 'Sem contatos para sondar.';

  @override
  String get groupInfo => 'Informações do grupo';

  @override
  String groupCreator(String name) {
    return 'Criado por $name';
  }

  @override
  String get seesYourIp => 'vê seu IP';

  @override
  String get ipWarningTitle => 'Atenção: IP visível no grupo';

  @override
  String get ipWarningBody =>
      'Neste grupo há pessoas que não são seus contatos. Elas passam a ver o seu endereço de rede (IP), e você o delas:';

  @override
  String joinedGroup(String name) {
    return 'Você entrou no grupo $name';
  }

  @override
  String removedFromGroup(String name) {
    return 'Você foi removido do grupo $name';
  }

  @override
  String get notInGroup => 'Você não está mais neste grupo.';

  @override
  String get addMember => 'Adicionar membro';

  @override
  String get removeMember => 'Remover';

  @override
  String get renameGroup => 'Renomear';

  @override
  String offlineMembers(String names) {
    return 'Offline agora: $names';
  }

  @override
  String errorGeneric(String error) {
    return 'Algo deu errado: $error';
  }

  @override
  String get navSettings => 'Configurações';

  @override
  String get attachFile => 'Enviar arquivo';

  @override
  String get dropHere => 'Solte o arquivo aqui para enviar';

  @override
  String get fileDownload => 'Baixar';

  @override
  String fileSavedAt(String path) {
    return 'Salvo em $path';
  }

  @override
  String fileSent(String name) {
    return 'Enviando $name…';
  }

  @override
  String fileFailed(String error) {
    return 'Falha com o arquivo: $error';
  }

  @override
  String get groupFilesUnsupported =>
      'Arquivos ainda não são suportados em grupos.';

  @override
  String get settingsTitle => 'Configurações';

  @override
  String get settingsBehavior => 'Comportamento';

  @override
  String get closeToTray => 'Fechar a janela mantém o Enxame ativo na bandeja';

  @override
  String get closeToTrayHelp =>
      'Assim seu computador continua ajudando seus contatos. Desligado, fechar a janela encerra o app.';

  @override
  String get autostart => 'Abrir o Enxame ao ligar o computador';

  @override
  String get autostartHelp => 'Desligado por padrão.';

  @override
  String get advancedTitle => 'Avançado';

  @override
  String get advancedHelp =>
      'Você não precisa mexer aqui. Estas opções são para quem usa um servidor próprio ou tem uma rede com problemas. Valem na próxima vez que o app abrir.';

  @override
  String get relayField => 'Relay (default, disabled ou URL do seu relay)';

  @override
  String get relayCaField => 'CA própria do relay (arquivo PEM, opcional)';

  @override
  String get dnsField =>
      'Servidores DNS (IPs separados por vírgula; vazio = automático)';

  @override
  String get downloadDirField => 'Pasta de downloads (vazio = padrão)';

  @override
  String get mdnsField => 'Descobrir contatos na rede local (mDNS)';

  @override
  String get saveSettings => 'Salvar';

  @override
  String get settingsSaved =>
      'Salvo. As mudanças valem quando você reabrir o app.';

  @override
  String get trayOpen => 'Abrir o Enxame';

  @override
  String get trayQuit => 'Sair';

  @override
  String trayRole(String role) {
    return 'Papel: $role';
  }

  @override
  String get trayUnavailable =>
      'Sua área de trabalho pode não mostrar o ícone da bandeja. Se não aparecer, desligue \"Fechar a janela mantém o Enxame ativo\".';

  @override
  String get callAudio => 'Chamada de voz';

  @override
  String get callVideo => 'Chamada de vídeo';

  @override
  String get callScreen => 'Transmitir minha tela';

  @override
  String callCalling(String name) {
    return 'Chamando $name…';
  }

  @override
  String callIncomingAudio(String name) {
    return '$name está te chamando (voz)';
  }

  @override
  String callIncomingVideo(String name) {
    return '$name está te chamando (vídeo)';
  }

  @override
  String callIncomingScreen(String name) {
    return '$name quer transmitir a tela para você';
  }

  @override
  String get callConnecting => 'Conectando…';

  @override
  String get callHangup => 'Desligar';

  @override
  String get callCancel => 'Cancelar';

  @override
  String get callMute => 'Silenciar microfone';

  @override
  String get callUnmute => 'Ativar microfone';

  @override
  String get callCameraOff => 'Desligar câmera';

  @override
  String get callCameraOn => 'Ligar câmera';

  @override
  String get callShareScreen => 'Compartilhar tela';

  @override
  String get callStopShare => 'Parar de compartilhar';

  @override
  String get callSharing => 'Você está transmitindo sua tela';

  @override
  String get callWaitingVideo => 'Aguardando o vídeo…';

  @override
  String callEnded(String reason) {
    return 'Chamada encerrada: $reason';
  }

  @override
  String get pickScreenTitle => 'O que você quer transmitir?';

  @override
  String get pickScreenScreens => 'Telas';

  @override
  String get pickScreenWindows => 'Janelas';

  @override
  String get pickScreenNone => 'Nenhuma tela encontrada.';

  @override
  String callPeerChoosing(String name) {
    return '$name está escolhendo o que transmitir…';
  }

  @override
  String callYouShareTo(String name) {
    return 'Você está transmitindo sua tela para $name';
  }

  @override
  String get callEndedPlain => 'Chamada encerrada';
}
